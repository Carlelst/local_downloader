use log::{error};
use reqwest::Client;
use tokio::sync::{mpsc, RwLock};
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use crate::downloader::download_with_retry;
use crate::decryptor;


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
/// 返回一个 Result，表示处理是否成功
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
    retry_interval: u64
) -> Result<()> {
    let data = download_with_retry(&client, &url, retries, timeout, retry_interval).await?;
    let processed_data = data.to_vec();
    let decrypted_data = if let (Some(key), Some(iv)) = (&aes_key, &aes_iv) {
        decryptor::decrypt_segment(&processed_data, key, iv).unwrap_or_else(|e| {
            error!("解密失败，段 {}: {:?}", idx, e);
            processed_data.clone()
        })
    } else {
        processed_data
    };

    segments.write().await.insert(idx, decrypted_data);
    notify_tx.send(idx).await.ok();
    Ok(())
}