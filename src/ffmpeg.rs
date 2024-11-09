use tokio::process::Command;
use anyhow::{Result, Context};
use std::process::Stdio;
use log::{info, debug, error};
use rand::{distributions::Alphanumeric, Rng};

/// 启动 FFmpeg 进程并返回子进程句柄。
/// 
/// # 返回
/// 返回 tokio::process::Child 对象，或在失败时返回错误。
pub async fn start_ffmpeg() -> Result<tokio::process::Child> {
    debug!("初始化 FFmpeg 进程");

    // 生成随机的输出文件名
    let random_filename: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10) // 随机字符串长度
        .map(char::from)
        .collect();
    let output_filename = format!("{}.mp4", random_filename);

    // 配置 FFmpeg 命令
    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("mpegts")
        .arg("-i")
        .arg("pipe:0")
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-shortest")
        .arg(&output_filename)
        .stdin(Stdio::piped())
        .spawn()
        .context("启动 FFmpeg 进程失败")?;

    info!("FFmpeg 进程已成功启动，输出文件: {}", output_filename);

    // 检查是否成功获取标准输入
    if ffmpeg.stdin.is_none() {
        error!("无法获取 FFmpeg 进程的标准输入");
        return Err(anyhow::anyhow!("无法获取 FFmpeg 进程的标准输入"));
    }

    Ok(ffmpeg)
}
