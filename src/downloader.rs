use reqwest::Client;
use tokio::time::{sleep, Duration};
use anyhow::Result;
use log::{error, warn};
use bytes::Bytes;

/// 通用的下载函数，增加详细日志和错误信息
pub async fn download_with_retry(client: &Client, url: &str, retries: u8, timeout: u64, retry_interval: u64) -> Result<Bytes> {
    for attempt in 0..=retries {
        // info!("Attempt {}: Downloading from {}", attempt + 1, url);
        match client.get(url).timeout(Duration::from_secs(timeout)).send().await {
            Ok(response) if response.status().is_success() => {
                // info!("Download successful for {}", url);
                return Ok(response.bytes().await?);
            },
            Ok(response) => {
                warn!("HTTP {} when downloading {}: Retrying...", response.status(), url);
            },
            Err(e) => {
                error!("Error on attempt {} for {}: {:?}", attempt + 1, url, e);
            }
        }
        if attempt < retries {
            sleep(Duration::from_secs(retry_interval)).await;
        }
    }
    Err(anyhow::anyhow!("Failed to download from {} after {} attempts", url, retries))
}


