use crate::Config;
use anyhow::{Context, Result};
use log::{debug, error, info};
use rand::{distributions::Alphanumeric, Rng};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

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
        let check = Command::new("ffmpeg")
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
///
/// # 返回
/// 返回 tokio::process::Child 对象，或在失败时返回错误。
pub async fn start_ffmpeg(config: &Config) -> Result<(tokio::process::Child, String)> {
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

    // 检测硬件加速
    let hw_accel = if config.use_hw_accel {
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

    let mut cmd = Command::new("ffmpeg");

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

    // 输入参数 - 添加更健壮的MPEG-TS处理参数
    cmd.arg("-f")
        .arg("mpegts")
        .arg("-analyzeduration")
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
    if config.video_codec != "copy" || hw_accel.is_none() {
        cmd.arg("-c:v").arg(&config.video_codec);
    } else if let Some(accel) = hw_accel {
        // 如果指定了copy但有硬件加速器，使用相应的硬件编码器
        match accel {
            "videotoolbox" => {
                info!("使用VideoToolbox硬件加速编码...");
                cmd.arg("-c:v").arg("h264_videotoolbox");
                // 如果没有设置比特率，添加默认比特率控制文件大小
                if config.video_bitrate.is_none() {
                    cmd.arg("-b:v").arg("2M");
                    info!("应用默认视频比特率: 2Mbps");
                }
            }
            "nvenc" => {
                info!("使用NVIDIA硬件加速编码...");
                cmd.arg("-c:v").arg("h264_nvenc");
                // 如果没有设置比特率，添加默认比特率控制文件大小
                if config.video_bitrate.is_none() {
                    cmd.arg("-b:v").arg("2M");
                    info!("应用默认视频比特率: 2Mbps");
                }
            }
            "qsv" => {
                info!("使用Intel QuickSync加速编码...");
                cmd.arg("-c:v").arg("h264_qsv");
                // 如果没有设置比特率，添加默认比特率控制文件大小
                if config.video_bitrate.is_none() {
                    cmd.arg("-b:v").arg("2M");
                    info!("应用默认视频比特率: 2Mbps");
                }
            }
            _ => {
                cmd.arg("-c:v").arg(&config.video_codec);
            }
        }
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
    cmd.arg("-movflags")
        .arg("+faststart")
        .arg("-shortest")
        .arg(&output_path_str);

    // 设置日志级别
    let log_level = match config.log_level.as_str() {
        "debug" => "info",
        "info" => "warning",
        "warn" | "warning" => "warning",
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
