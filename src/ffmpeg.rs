use crate::{m3u8_parser::SegmentFormat, Config};
use anyhow::{Context, Result};
use log::{debug, error, info};
use rand::{distributions::Alphanumeric, Rng};
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::process::Command;

/// ffmpeg 可执行文件路径（进程内缓存，首次调用时解析）
static FFMPEG_BIN: OnceLock<String> = OnceLock::new();

/// 解析 ffmpeg 路径：launchd 等受限 PATH 环境（只有 /usr/bin:/bin）下
/// `Command::new("ffmpeg")` 会直接 ENOENT，必须 fallback 到常见安装位置
pub fn ffmpeg_bin() -> String {
    FFMPEG_BIN
        .get_or_init(|| {
            // 1) PATH 中查找（CLI / 交互 shell 环境）
            if let Ok(out) = std::process::Command::new("which").arg("ffmpeg").output() {
                if out.status.success() {
                    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !p.is_empty() {
                        return p;
                    }
                }
            }
            // 2) launchd 等受限环境的常见安装位置
            for c in [
                "/opt/homebrew/bin/ffmpeg",
                "/usr/local/bin/ffmpeg",
                "/usr/bin/ffmpeg",
            ] {
                if Path::new(c).exists() {
                    return c.to_string();
                }
            }
            // 3) 兜底交给系统解析（报错信息保持原样便于排查）
            "ffmpeg".to_string()
        })
        .clone()
}

/// 检测系统中可用的硬件加速器
async fn detect_hw_accel() -> Option<&'static str> {
    // 简化硬件加速检测，只检测几个常见的
    let platforms = [
        "videotoolbox", // macOS
        "nvenc",        // NVIDIA GPU
        "qsv",          // Intel QuickSync
        "cuda",         // NVIDIA CUDA
    ];

    for name in platforms.iter() {
        let check = Command::new(ffmpeg_bin())
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-hwaccel")
            .arg(name)
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("nullsrc=s=1280x720:d=1")
            .arg("-an")
            .arg("-f")
            .arg("null")
            .arg("-")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        if let Ok(status) = check {
            if status.success() {
                info!("检测到硬件加速器: {}", name);
                return Some(name);
            }
        }
    }

    debug!("未检测到支持的硬件加速器，使用软件解码");
    None
}

/// 启动 FFmpeg 进程并返回子进程句柄。
///
/// # 参数
/// - `config`: 应用程序配置
/// - `segment_format`: 片段格式（MPEG-TS 或 fMP4）
///
/// # 返回
/// 返回 tokio::process::Child 对象，或在失败时返回错误。
pub async fn start_ffmpeg(
    config: &Config,
    segment_format: SegmentFormat,
) -> Result<(tokio::process::Child, String)> {
    debug!("初始化 FFmpeg 进程");

    // 确保输出目录存在
    if !Path::new(&config.output_directory).exists() {
        tokio::fs::create_dir_all(&config.output_directory)
            .await
            .context("创建输出目录失败")?;
    }

    // 生成随机的输出文件名
    let random_filename: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10) // 随机字符串长度
        .map(char::from)
        .collect();
    let output_filename = format!("{}.{}", random_filename, config.video_format);
    let output_path = Path::new(&config.output_directory).join(&output_filename);
    let output_path_str = format!("{}", output_path.display());

    // 检测硬件加速（流复制 remux 用不到编码器，跳过检测——每次探测要起 ffmpeg 子进程跑约 1s）
    let hw_accel = if config.use_hw_accel && config.video_codec != "copy" {
        detect_hw_accel().await
    } else {
        None
    };

    // 确定线程数
    let thread_count = if config.thread_count > 0 {
        config.thread_count
    } else {
        // 默认使用系统的CPU核心数
        let cores = num_cpus::get();
        let threads = std::cmp::max(1, cores.saturating_sub(1));
        debug!("自动设置线程数: {}", threads);
        threads as i32
    };

    let mut cmd = Command::new(ffmpeg_bin());

    // 基本参数
    cmd.arg("-y");

    // 添加硬件加速参数（如果可用）
    if let Some(accel) = hw_accel {
        match accel {
            "videotoolbox" => {
                cmd.arg("-hwaccel").arg("videotoolbox");
            }
            "nvenc" => {
                cmd.arg("-hwaccel").arg("cuda");
            }
            "qsv" => {
                cmd.arg("-hwaccel")
                    .arg("qsv")
                    .arg("-hwaccel_output_format")
                    .arg("qsv");
            }
            "cuda" => {
                cmd.arg("-hwaccel")
                    .arg("cuda")
                    .arg("-hwaccel_output_format")
                    .arg("cuda");
            }
            _ => {
                cmd.arg("-hwaccel").arg(accel);
            }
        }
    }

    // 根据片段格式设置输入格式
    // TS 显式指定 mpegts；fMP4 不指定 -f（"movfrag" 并非有效的 ffmpeg 输入格式），
    // 让 ffmpeg 自动探测——init 段的 ftyp box 会被 mov/mp4 demuxer 正确识别
    if segment_format == SegmentFormat::Ts {
        info!("检测到 MPEG-TS 格式");
        cmd.arg("-f").arg("mpegts");
    } else {
        info!("检测到 fMP4 格式，使用自动探测输入");
    }
    cmd.arg("-analyzeduration")
        .arg("1000000") // 增加分析时长
        .arg("-probesize")
        .arg("1000000") // 增加探测大小
        .arg("-i")
        .arg("pipe:0");

    // 设置线程数
    cmd.arg("-threads").arg(thread_count.to_string());

    // 处理时间戳不连续问题的参数 - 添加更健壮的错误恢复
    cmd.arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-max_interleave_delta")
        .arg("0")
        .arg("-fflags")
        .arg("+genpts+igndts+discardcorrupt")
        .arg("-err_detect")
        .arg("ignore_err"); // 忽略错误以提高鲁棒性

    // 视频和音频编码参数
    // video_codec=copy 时严格流复制（TS→MP4 纯 remux，仅受 IO 限制，1GB 视频几秒完成）。
    // 绝不能因检测到硬件加速器就把 copy 替换成编码器——那是反优化：
    // 全片重编码耗时暴涨数十倍，且默认 2Mbps 比特率会严重降质
    if config.video_codec == "copy" {
        cmd.arg("-c:v").arg("copy");
    } else {
        cmd.arg("-c:v").arg(&config.video_codec);
    }

    // 设置比特率（如果指定）
    if let Some(ref bitrate) = config.video_bitrate {
        cmd.arg("-b:v").arg(bitrate);
    }

    // 设置音频编码
    cmd.arg("-c:a").arg(&config.audio_codec);

    // 如果指定了音频比特率
    if let Some(ref audio_bitrate) = config.audio_bitrate {
        cmd.arg("-b:a").arg(audio_bitrate);
    }

    // 优化输出
    // 不用 +faststart：它会在写完后把 moov box 移到文件头，需要二次全文件重写，
    // 200MB 视频实测多花数分钟。本工具产出的是本地完整文件（先下载完再 remux），
    // moov 在尾部对本地播放（VLC/IINA/mpv/浏览器 file://）无影响；
    // 若需要网页边下边播，在输出命令后手动执行 ffmpeg -i in.mp4 -c copy -movflags +faststart out.mp4
    cmd.arg("-shortest").arg(&output_path_str);

    // 设置日志级别（ffmpeg 的 info 会直接向终端刷 progress 行，混入日志输出，故最高只给 warning）
    let log_level = match config.log_level.as_str() {
        "info" | "warn" | "warning" | "debug" | "trace" => "warning",
        "error" => "error",
        _ => "warning",
    };

    cmd.arg("-loglevel").arg(log_level);

    // 执行命令
    debug!("FFmpeg命令: {:?}", cmd);

    let ffmpeg = cmd
        .stdin(Stdio::piped())
        .spawn()
        .context("启动 FFmpeg 进程失败")?;

    info!("FFmpeg 进程已成功启动，输出文件: {}", output_path_str);

    // 检查是否成功获取标准输入
    if ffmpeg.stdin.is_none() {
        error!("无法获取 FFmpeg 进程的标准输入");
        return Err(anyhow::anyhow!("无法获取 FFmpeg 进程的标准输入"));
    }

    Ok((ffmpeg, output_path_str))
}
