use anyhow::{Context, Result};
use bytes::Bytes;
use log::{debug, error, warn};
use rand::Rng;
use reqwest::{Client, StatusCode};
use std::time::Instant;
use tokio::time::{sleep, Duration};

/// 下载具有重试机制的段数据
///
/// # 参数
/// - `client`: HTTP 客户端
/// - `url`: 下载的 URL
/// - `retries`: 最大重试次数
/// - `timeout`: 每次请求的超时时间（秒）
/// - `retry_interval`: 每次重试之间的基础间隔时间（秒）
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
    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(timeout);

    for attempt in 0..=retries {
        let total_attempts = retries + 1;
        debug!(
            "尝试下载 (尝试次数: {}/{}), url: {}",
            attempt + 1,
            total_attempts,
            url
        );

        // 计算当前尝试的超时时间（保证总超时不超过原始超时）
        let elapsed = start_time.elapsed();
        let remaining = if elapsed < timeout_duration {
            timeout_duration - elapsed
        } else {
            Duration::from_millis(500) // 最小超时保底
        };

        match client.get(url).timeout(remaining).send().await {
            Ok(response) => {
                let status = response.status();

                match status {
                    StatusCode::OK => {
                        debug!(
                            "成功下载: {}, 大小: {}字节",
                            url,
                            response.content_length().unwrap_or(0)
                        );
                        return response.bytes().await.context("读取响应字节失败");
                    }
                    // 服务器端错误，可重试
                    s if s.is_server_error() => {
                        warn!("服务器错误 {} 接收自 {}: 重试中...", status, url);
                    }
                    // 客户端错误，通常不重试（认证问题、404等）
                    s if s.is_client_error() => {
                        if status == StatusCode::TOO_MANY_REQUESTS
                            || status == StatusCode::REQUEST_TIMEOUT
                        {
                            warn!(
                                "请求限制或超时 {} 接收自 {}: 增加等待时间后重试...",
                                status, url
                            );
                        } else {
                            error!("客户端错误 {} 接收自 {}: 可能是永久性错误", status, url);
                            // 对于404等明确的错误，直接失败可能更好
                            if status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN {
                                return Err(anyhow::anyhow!(
                                    "资源不可访问，HTTP状态码: {}",
                                    status
                                ));
                            }
                        }
                    }
                    // 其他状态码
                    _ => {
                        warn!("未预期的HTTP状态码 {} 接收自 {}: 重试中...", status, url);
                    }
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    warn!("第 {} 次尝试下载 {} 时超时", attempt + 1, url);
                } else if e.is_connect() {
                    warn!("第 {} 次尝试下载 {} 时连接失败: {:?}", attempt + 1, url, e);
                } else {
                    error!("第 {} 次尝试下载 {} 时出错: {:?}", attempt + 1, url, e);
                }
            }
        }

        if attempt < retries {
            // 使用指数退避策略，重试间隔随重试次数增加而增加
            let base_delay = retry_interval * 2u64.pow(attempt as u32);
            // 增加随机抖动，避免多个请求同时重试导致的雪崩
            let jitter_ms = rand::thread_rng().gen_range(0..=1000);
            let delay_ms = (base_delay * 1000 + jitter_ms).min(30000); // 最大延迟30秒

            debug!("等待 {} 毫秒后进行第 {} 次重试", delay_ms, attempt + 2);
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let elapsed = start_time.elapsed();
    Err(anyhow::anyhow!(
        "在 {} 次尝试后无法下载: {}，总耗时: {:.2?}",
        retries + 1,
        url,
        elapsed
    ))
}
