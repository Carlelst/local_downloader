use reqwest::Client;
use tokio::time::{sleep, Duration};
use anyhow::{Result, Context};
use log::{error, warn, debug};
use bytes::Bytes;
use rand::Rng;

/// 下载具有重试机制的段数据
/// 
/// # 参数
/// - `client`: HTTP 客户端
/// - `url`: 下载的 URL
/// - `retries`: 最大重试次数
/// - `timeout`: 每次请求的超时时间（秒）
/// - `retry_interval`: 每次重试之间的间隔时间（秒）
/// 
/// # 返回
/// 返回下载的字节数据，或在失败时返回错误。
pub async fn download_with_retry(
    client: &Client,
    url: &str,
    retries: u8,
    timeout: u64,
    retry_interval: u64,
) -> Result<Bytes> {
    for attempt in 0..=retries {
        debug!("尝试下载 (尝试次数: {}/{})", attempt + 1, retries + 1);
        match client.get(url).timeout(Duration::from_secs(timeout)).send().await {
            Ok(response) if response.status().is_success() => {
                debug!("成功下载: {}", url);
                return response.bytes().await.context("读取响应字节失败");
            }
            Ok(response) => {
                warn!("HTTP 状态码 {} 接收自 {}: 重试中...", response.status(), url);
            }
            Err(e) => {
                error!("第 {} 次尝试下载 {} 时出错: {:?}", attempt + 1, url, e);
            }
        }
        
        if attempt < retries {
            let jitter: u64 = rand::thread_rng().gen_range(0..=2);
            debug!("等待 {} 秒后重试 (含抖动: {} 秒)", retry_interval, jitter);
            sleep(Duration::from_secs(retry_interval + jitter)).await;
        } else {
            error!("已达到最大重试次数，无法下载: {}", url);
        }
    }
    
    Err(anyhow::anyhow!("重试 {} 次后无法下载: {}", retries, url))
}
