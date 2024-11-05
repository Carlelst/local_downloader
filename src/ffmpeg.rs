use tokio::process::Command;
use anyhow::Result;
use std::process::Stdio;

/// 启动 FFmpeg 进程以接收解密后的流数据
pub async fn start_ffmpeg() -> Result<tokio::process::Child> {
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
        .arg("output.mp4")
        .stdin(Stdio::piped())
        .spawn()?;
    Ok(ffmpeg)
}
