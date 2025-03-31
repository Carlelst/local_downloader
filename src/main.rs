use anyhow::Result;
use chrono::Local;
use env_logger::Builder;
use log::{error, info};
use reqwest::Client;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::Instant;

use video_processor::{
    ffmpeg::start_ffmpeg, m3u8_parser::parse_m3u8, parse_log_level,
    segment_processor::process_segment,
};

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    let config = video_processor::load_config()?;

    // 初始化日志
    Builder::new()
        .filter(None, parse_log_level(&config.log_level))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();

    info!("加载配置: {:?}", config);
    info!("使用并发下载数: {}", config.max_concurrent_downloads);

    let client = Arc::new(Client::new());

    for m3u8_url in config.m3u8_urls.iter() {
        if m3u8_url.trim().is_empty() {
            continue;
        }

        info!("处理 M3U8 文件: {}", m3u8_url);

        let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
        let total_segments = ts_urls.len();

        if total_segments == 0 {
            error!("未找到视频片段，跳过处理");
            continue;
        }

        info!("找到 {} 个视频片段，开始下载", total_segments);

        let segments = Arc::new(RwLock::new(HashMap::<usize, Vec<u8>>::with_capacity(
            total_segments,
        )));
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads));
        let (notify_tx, mut notify_rx) = mpsc::channel(total_segments);

        let (mut ffmpeg, output_file) = start_ffmpeg(&config).await?;
        let ffmpeg_stdin = ffmpeg
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("无法打开 FFmpeg 标准输入"))?;
        let mut buffered_stdin = BufWriter::with_capacity(config.buffer_size, ffmpeg_stdin);

        // 启动段下载任务
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
                if let Err(e) = process_segment(
                    client,
                    url,
                    idx,
                    aes_key,
                    aes_iv,
                    segments,
                    notify_tx,
                    timeout,
                    retries,
                    retry_interval,
                )
                .await
                {
                    error!("段处理失败，段 {}: {:?}", idx, e);
                }
            });
        }
        drop(notify_tx); // 关闭发送端

        // 进度追踪
        let mut processed_segments = 0;
        let mut next_idx = 0;

        while let Some(_) = notify_rx.recv().await {
            processed_segments += 1;
            if processed_segments % 10 == 0 || processed_segments == total_segments {
                info!(
                    "进度: {}/{} ({:.1}%)",
                    processed_segments,
                    total_segments,
                    (processed_segments as f64 / total_segments as f64) * 100.0
                );
            }

            let mut segments_lock = segments.write().await;
            while let Some(data) = segments_lock.remove(&next_idx) {
                buffered_stdin.write_all(&data).await?;
                next_idx += 1;
            }
        }

        // 确保所有数据都被写入
        buffered_stdin.flush().await?;
        drop(buffered_stdin);

        if ffmpeg.wait().await?.success() {
            info!("FFmpeg 处理成功: {}", output_file);
        } else {
            error!("FFmpeg 处理失败: {}", output_file);
        }
    }

    info!("总耗时: {:.2?}", start_time.elapsed());
    Ok(())
}
