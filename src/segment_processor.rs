use crate::decryptor;
use crate::downloader::download_with_retry;
use anyhow::Result;
use log::{debug, error};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// 处理单个段的下载和解密
///
/// # 参数
/// - `client`: HTTP 客户端实例
/// - `url`: 段的 URL
/// - `idx`: 段的索引
/// - `aes_key`: 可选的 AES 解密密钥
/// - `aes_iv`: 可选的 AES 初始化向量
/// - `segments`: 存储段数据的共享可变引用
/// - `notify_tx`: 用于通知处理完成的通道发送端
/// - `timeout`: 每次请求的超时时间（秒）
/// - `retries`: 最大重试次数
/// - `retry_interval`: 重试之间的间隔时间（秒）
///
/// # 返回
/// 返回一个 Result，包含解密后的数据或错误
pub async fn process_segment(
    client: Arc<Client>,
    url: String,
    idx: usize,
    aes_key: Option<Vec<u8>>,
    aes_iv: Option<Vec<u8>>,
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>,
    notify_tx: mpsc::Sender<usize>,
    timeout: u64,
    retries: u8,
    retry_interval: u64,
) -> Result<Vec<u8>> {
    debug!("开始处理片段 {}: {}", idx, url);
    let data = download_with_retry(&client, &url, retries, timeout, retry_interval).await?;
    let processed_data = data.to_vec();
    let decrypted_data = if let (Some(key), Some(iv)) = (&aes_key, &aes_iv) {
        match decryptor::decrypt_segment(&processed_data, key, iv) {
            Ok(data) => {
                debug!("片段 {} 解密成功，大小: {} 字节", idx, data.len());
                data
            }
            Err(e) => {
                error!("解密失败，段 {}: {:?}", idx, e);
                processed_data.clone()
            }
        }
    } else {
        debug!(
            "片段 {} 不需要解密，大小: {} 字节",
            idx,
            processed_data.len()
        );
        processed_data
    };

    // 将解密后的数据存入共享数据结构并通知
    segments.write().await.insert(idx, decrypted_data.clone());
    if let Err(e) = notify_tx.send(idx).await {
        error!("无法发送通知，片段 {}: {:?}", idx, e);
    }

    // 返回解密后的数据，这样调用者可以保存到文件
    Ok(decrypted_data)
}
