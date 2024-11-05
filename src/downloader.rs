use reqwest::Client;
use tokio::time::sleep;
use log::{error, info};
use std::time::Duration;
use anyhow::Result;

/// 使用重试机制下载数据
pub async fn download_with_retry(client: &Client, url: &str, retries: u8) -> Result<Vec<u8>> {
    let mut attempt = 0;
    while attempt <= retries {
        match client.get(url).timeout(Duration::from_secs(40)).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(response.bytes().await?.to_vec());
            }
            Ok(response) => {
                error!("Failed to download segment {}: HTTP {}", url, response.status());
            }
            Err(e) => {
                error!("Error downloading segment {}: {:?}", url, e);
            }
        }
        attempt += 1;
        if attempt <= retries {
            info!("Retrying download for {} (attempt {}/{})", url, attempt, retries);
            sleep(Duration::from_secs(3)).await;
        }
    }
    Err(anyhow::anyhow!("Failed to download segment from {} after {} retries", url, retries))
}
