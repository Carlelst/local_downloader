use reqwest::Client;
use tokio::time::{sleep, Duration};
use anyhow::{Result, Context};
use log::{error, warn};
use bytes::Bytes;
use rand::Rng;

pub async fn download_with_retry(
    client: &Client,
    url: &str,
    retries: u8,
    timeout: u64,
    retry_interval: u64,
) -> Result<Bytes> {
    for attempt in 0..=retries {
        match client.get(url).timeout(Duration::from_secs(timeout)).send().await {
            Ok(response) if response.status().is_success() => {
                return response.bytes().await.context("Failed to read response bytes");
            }
            Ok(response) => {
                warn!("HTTP {} received when downloading {}: Retrying...", response.status(), url);
            }
            Err(e) => {
                error!("Error on attempt {} for {}: {:?}", attempt + 1, url, e);
            }
        }
        if attempt < retries {
            let jitter: u64 = rand::thread_rng().gen_range(0..=2);
            sleep(Duration::from_secs(retry_interval + jitter)).await;
        }
    }
    Err(anyhow::anyhow!("Failed to download from {} after {} attempts", url, retries))
}