use tokio::process::Command;
use anyhow::{Result, Context};
use std::process::Stdio;
use log::{info, debug};
use rand::{distributions::Alphanumeric, Rng};

pub async fn start_ffmpeg() -> Result<tokio::process::Child> {
    debug!("Initializing FFmpeg process");

    // Generate a random output file name
    let random_filename: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10) // Length of the random string
        .map(char::from)
        .collect();
    let output_filename = format!("{}.mp4", random_filename);

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
        .context("Failed to start FFmpeg process")?;

    info!("FFmpeg process started successfully, output file: {}", output_filename);
    Ok(ffmpeg)
}
