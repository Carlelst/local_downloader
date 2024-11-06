use env_logger::Builder;
use log::{info, error, LevelFilter};
use tokio::sync::{mpsc, Semaphore, RwLock};
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use reqwest::Client;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, Instant};
use chrono::Local;
use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;
use std::io::Write;

use video_processor::m3u8_parser::parse_m3u8;
use video_processor::ffmpeg::start_ffmpeg;

mod decryptor;

const MAX_CONCURRENT_DOWNLOADS: usize = 8;

async fn download_with_retry(client: &Client, url: &str, retries: u8) -> Result<Bytes> {
    for attempt in 0..=retries {
        match client.get(url).timeout(Duration::from_secs(30)).send().await {
            Ok(response) if response.status().is_success() => return Ok(response.bytes().await?),
            Ok(response) => error!("Failed to download segment {}: HTTP {}", url, response.status()),
            Err(e) => error!("Error downloading segment {}: {:?}", url, e),
        }
        if attempt < retries {
            info!("Retrying download for {} (attempt {}/{})", url, attempt + 1, retries);
            sleep(Duration::from_secs(3)).await;
        }
    }
    Err(anyhow::anyhow!("Failed to download segment from {} after {} retries", url, retries))
}

async fn convert_jpeg_to_ts_pipe(input_data: &[u8], duration: u32) -> Result<Vec<u8>> {
    let mut ffmpeg_process = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f").arg("image2pipe")
        .arg("-i").arg("pipe:0")
        .arg("-t").arg(duration.to_string())
        .arg("-c:v").arg("libx264")
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-f").arg("mpegts")
        .arg("pipe:1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    
    let stdin = ffmpeg_process.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("Failed to open ffmpeg stdin"))?;
    stdin.write_all(input_data).await?;
    stdin.shutdown().await?;
    
    let output = ffmpeg_process.wait_with_output().await?;
    Ok(output.stdout)
}

async fn process_segment(
    client: Arc<Client>, url: String, idx: usize, aes_key: Option<Vec<u8>>, aes_iv: Option<Vec<u8>>, 
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>, notify_tx: mpsc::Sender<usize>
) -> Result<()> {
    let data = download_with_retry(&client, &url, 10).await?;
    let processed_data = if url.ends_with(".jpeg") {
        convert_jpeg_to_ts_pipe(&data, 4).await?
    } else {
        data.to_vec()
    };
    let decrypted_data = if let (Some(key), Some(iv)) = (&aes_key, &aes_iv) {
        decryptor::decrypt_segment(&processed_data, key, iv).unwrap_or_else(|e| {
            error!("Decryption failed for segment {}: {:?}", idx, e);
            processed_data.clone()
        })
    } else {
        processed_data
    };
    segments.write().await.insert(idx, decrypted_data);
    notify_tx.send(idx).await.ok();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now(); // 记录程序开始时间

    Builder::new()
        .filter(None, LevelFilter::Info)
        .format(|buf, record| writeln!(buf, "[{} {}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), record.level(), record.args()))
        .init();

    let client = Arc::new(Client::new());
    let m3u8_url = "https://hls-uranus.sb-cd.com/hls/1/3/13048167-480p.mp4/index-v1-a1.m3u8?_tid=13048167&d=1&m=48&secure=_w8HICUxls0h6jhDBkZGKA,1730940604";
    let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
    let total_segments = ts_urls.len();
    let segments = Arc::new(RwLock::new(HashMap::with_capacity(total_segments)));
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let (notify_tx, mut notify_rx) = mpsc::channel(total_segments);
    let mut ffmpeg = start_ffmpeg().await?;
    let ffmpeg_stdin = ffmpeg.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("Unable to open FFmpeg stdin"))?;

    for (idx, url) in ts_urls.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let segments = Arc::clone(&segments);
        let notify_tx = notify_tx.clone();
        let aes_key = aes_key.clone();
        let aes_iv = aes_iv.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            process_segment(client, url, idx, aes_key, aes_iv, segments, notify_tx).await.ok();
        });
    }
    drop(notify_tx);

    let mut next_idx = 0;
    while let Some(_) = notify_rx.recv().await {
        let mut segments_lock = segments.write().await;
        while let Some(data) = segments_lock.remove(&next_idx) {
            ffmpeg_stdin.write_all(&data).await?;
            next_idx += 1;
        }
    }
    ffmpeg_stdin.shutdown().await?;
    if ffmpeg.wait().await?.success() {
        info!("FFmpeg processing completed");
    } else {
        error!("FFmpeg processing failed");
    }

    let elapsed = start_time.elapsed(); // 计算总耗时
    info!("Total execution time: {:.2?}", elapsed);

    Ok(())
}
