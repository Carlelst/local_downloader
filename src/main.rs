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

use video_processor::{
    m3u8_parser::parse_m3u8,
    ffmpeg::start_ffmpeg,
    segment_processor::process_segment,
};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    Builder::new()
        .filter(None, LevelFilter::Info)
        .format(|buf, record| writeln!(buf, "[{} {}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), record.level(), record.args()))
        .init();

    let start_time = Instant::now(); // 记录程序开始时间

    // 加载配置
    let config = video_processor::load_config()?;
    info!("加载配置: {:?}", config);

    let client = Arc::new(Client::new());
    let (ts_urls, aes_key, aes_iv) = parse_m3u8(&config.m3u8_url, &client).await?;
    let total_segments = ts_urls.len();
    let segments = Arc::new(RwLock::new(HashMap::<usize, Vec<u8>>::with_capacity(total_segments)));
    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads));
    let (notify_tx, mut notify_rx) = mpsc::channel(total_segments);
    let mut ffmpeg = start_ffmpeg().await?;
    let ffmpeg_stdin = ffmpeg.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("无法打开 FFmpeg 标准输入"))?;

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
                error!("段处理失败，段 {}: {:?}", idx, e);
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
        info!("FFmpeg 处理成功");
    } else {
        error!("FFmpeg 处理失败");
    }

    info!("总耗时: {:.2?}", start_time.elapsed());
    Ok(())
}
