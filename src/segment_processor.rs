use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

use crate::{decrypt_segment, download_with_retry};

const REPORT_INTERVAL: Duration = Duration::from_secs(30);

// 使用Mutex替代unsafe静态变量
lazy_static::lazy_static! {
    static ref STATS: Mutex<Stats> = Mutex::new(Stats {
        last_report_time: None,
        total_processed: 0,
        total_size: 0,
    });
}

struct Stats {
    last_report_time: Option<Instant>,
    total_processed: usize,
    total_size: usize,
}

/// 处理一个视频片段：下载、解密并将结果保存到共享映射中
///
/// # 参数
/// - `client`: HTTP客户端
/// - `url`: 片段URL
/// - `idx`: 片段索引
/// - `aes_key`: 可选的AES密钥，用于解密
/// - `aes_iv`: 可选的AES初始化向量，用于解密
/// - `segments`: 共享的片段存储
/// - `notify_tx`: 用于通知处理完成的通道
/// - `timeout`: 超时（秒）
/// - `retries`: 重试次数
/// - `retry_interval`: 重试间隔（秒）
///
/// # 返回
/// 成功时返回处理后的数据
pub async fn process_segment(
    client: Arc<Client>,
    url: String,
    idx: usize,
    aes_key: Option<Vec<u8>>,
    aes_iv: Option<Vec<u8>>,
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>,
    notify_tx: Sender<usize>,
    timeout: u64,
    retries: u8,
    retry_interval: u64,
) -> Result<Vec<u8>> {
    let start_time = Instant::now();
    debug!("开始处理片段 {}: {}", idx, url);

    // 创建下载进度回调
    let download_start = Instant::now();
    let last_progress_time = Arc::new(Mutex::new(Instant::now()));
    let downloaded_bytes = Arc::new(Mutex::new(0usize));

    let downloaded_bytes_clone = Arc::clone(&downloaded_bytes);
    let last_time_clone = Arc::clone(&last_progress_time);

    let progress_callback = Box::new(move |offset: usize, chunk_size: usize, _chunk: &[u8]| {
        let mut bytes = downloaded_bytes_clone.lock().unwrap();
        *bytes = offset + chunk_size;

        // 每5秒更新一次进度，避免日志过多
        let mut last_time = last_time_clone.lock().unwrap();
        if last_time.elapsed() > Duration::from_secs(5) {
            let elapsed = download_start.elapsed();
            let speed = if elapsed.as_millis() > 0 {
                *bytes as f64 / elapsed.as_millis() as f64 * 1000.0 / 1024.0
            } else {
                0.0
            };
            debug!("片段 {} 下载进度: {} 字节, {:.2} KB/s", idx, *bytes, speed);
            *last_time = Instant::now();
        }
    });

    // 下载片段
    let data = download_with_retry(
        &client,
        &url,
        timeout,
        retries,
        retry_interval,
        Some(progress_callback),
    )
    .await
    .map_err(|e| {
        error!("下载片段 {} 失败: {}", idx, e);
        anyhow!("下载片段失败 ({}): {}", idx, e)
    })?;

    // 更新统计信息
    {
        let mut stats = STATS.lock().unwrap();
        stats.total_size += data.len();
        stats.total_processed += 1;

        // 如果需要，打印定期报告
        let now = Instant::now();
        if stats.last_report_time.is_none() {
            stats.last_report_time = Some(now);
        } else if let Some(last) = stats.last_report_time {
            if now.duration_since(last) >= REPORT_INTERVAL {
                info!(
                    "性能报告: 已处理 {} 个片段, 总大小 {:.2} MB",
                    stats.total_processed,
                    stats.total_size as f64 / (1024.0 * 1024.0)
                );
                stats.last_report_time = Some(now);
            }
        }
    }

    // 解密数据（如果需要）
    let processed_data = if let (Some(key), Some(iv)) = (&aes_key, &aes_iv) {
        debug!("解密片段 {}, 大小: {} 字节", idx, data.len());
        match decrypt_segment(&data, key, iv) {
            Ok(decrypted) => {
                debug!(
                    "解密成功，片段 {} 解密后大小: {} 字节",
                    idx,
                    decrypted.len()
                );
                decrypted
            }
            Err(e) => {
                warn!("解密片段 {} 失败: {}，使用原始数据", idx, e);
                data.clone()
            }
        }
    } else {
        debug!("片段 {} 不需要解密", idx);
        data.clone()
    };

    // 注意片段大小
    debug!(
        "片段 {} 处理完成，处理前: {} 字节，处理后: {} 字节，用时: {:.2?}",
        idx,
        data.len(),
        processed_data.len(),
        start_time.elapsed()
    );

    // 将处理后的数据插入共享Map
    {
        let mut segments_lock = segments.write().await;
        segments_lock.insert(idx, processed_data.clone());
    }

    // 通知处理完成
    if let Err(e) = notify_tx.send(idx).await {
        warn!("无法发送完成通知: {}, idx: {}", e, idx);
    }

    Ok(processed_data)
}
