use env_logger::Builder;
use log::{info, error, LevelFilter};
use tokio::sync::{mpsc, Semaphore, RwLock};
use tokio::io::AsyncWriteExt;
use reqwest::Client;
use std::sync::Arc;
use tokio::time::Instant;
use chrono::Local;
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;
use serde::Deserialize;
use serde_yaml::from_reader;
use std::fs::File;
use anyhow::Context;

use video_processor::m3u8_parser::parse_m3u8;
use video_processor::ffmpeg::start_ffmpeg;
use video_processor::downloader::download_with_retry;

mod decryptor;

#[derive(Debug, Deserialize)]
struct Config {
    m3u8_url: String,
    max_concurrent_downloads: usize,
    download_retries: u8,
    segment_timeout: u64,
    retry_interval: u64,
}

// 加载 YAML 配置文件
fn load_config() -> Result<Config> {
    let file = File::open("config.yaml").context("Failed to open config file")?;
    let config: Config = from_reader(file).context("Failed to parse config file")?;
    Ok(config)
}

/// 处理单个段的下载和解密
async fn process_segment(
    client: Arc<Client>, 
    url: String, 
    idx: usize, 
    aes_key: Option<Vec<u8>>, 
    aes_iv: Option<Vec<u8>>,
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>, 
    notify_tx: mpsc::Sender<usize>,
    timeout: u64, 
    retries: u8, 
    retry_interval: u64
) -> Result<()> {
    let data = download_with_retry(&client, &url, retries, timeout, retry_interval).await?;
    let processed_data = data.to_vec();
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
    Builder::new()
        .filter(None, LevelFilter::Info)
        .format(|buf, record| writeln!(buf, "[{} {}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), record.level(), record.args()))
        .init();

    let start_time = Instant::now(); // 记录程序开始时间

    let config = load_config()?;
    info!("Loaded configuration: {:?}", config);

    let client = Arc::new(Client::new());
    let (ts_urls, aes_key, aes_iv) = parse_m3u8(&config.m3u8_url, &client).await?;
    let total_segments = ts_urls.len();
    let segments = Arc::new(RwLock::new(HashMap::with_capacity(total_segments)));
    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads));
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
        let timeout = config.segment_timeout;
        let retries = config.download_retries;
        let retry_interval = config.retry_interval;

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            if let Err(e) = process_segment(client, url, idx, aes_key, aes_iv, segments, notify_tx, timeout, retries, retry_interval).await {
                error!("Segment processing failed for segment {}: {:?}", idx, e);
            }
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
        info!("FFmpeg processing completed successfully");
    } else {
        error!("FFmpeg processing failed");
    }

    info!("Total Time: {:.2?}", start_time.elapsed());
    Ok(())
}
