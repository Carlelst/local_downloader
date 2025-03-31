use crate::Config;
use anyhow::{Context, Result};
use log::{debug, error, info};
use rand::{distributions::Alphanumeric, Rng};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

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

    // 配置 FFmpeg 命令
    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("mpegts")
        .arg("-i")
        .arg("pipe:0")
        .arg("-c:v")
        .arg(&config.video_codec)
        .arg("-c:a")
        .arg(&config.audio_codec)
        .arg("-shortest")
        .arg(&output_path_str)
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
