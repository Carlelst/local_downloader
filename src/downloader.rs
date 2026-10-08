use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use log::{trace, warn};
use reqwest::Client;
use std::time::Duration;
use tokio::time::{sleep, timeout, Instant};

/// 下载URL内容，支持重试和超时
///
/// # 参数
/// - `client`: HTTP客户端
/// - `url`: 要下载的URL
/// - `timeout_secs`: 超时秒数
/// - `retries`: 重试次数
/// - `retry_interval`: 重试间隔秒数
/// - `chunk_callback`: 可选的回调函数，用于处理下载的数据块
///
/// # 返回
/// 成功时返回下载的数据
pub async fn download_with_retry(
    client: &Client,
    url: &str,
    timeout_secs: u64,
    retries: u8,
    retry_interval: u64,
    chunk_callback: Option<Box<dyn Fn(usize, usize, &[u8]) + Send + Sync>>,
) -> Result<Vec<u8>> {
    let mut retry_count = 0;
    let mut last_error = None;
    let mut backoff_interval = retry_interval;
    let timeout_duration = Duration::from_secs(timeout_secs);

    // 计算一个随机的起始延迟，避免多个下载任务同时开始
    let initial_delay = rand::random::<u64>() % 500; // 0-500ms的随机延迟
    if initial_delay > 0 {
        sleep(Duration::from_millis(initial_delay)).await;
    }

    while retry_count <= retries {
        let start_time = Instant::now();
        let download_result = if chunk_callback.is_some() {
            // 使用流式下载
            stream_download(client, url, timeout_duration, chunk_callback.as_ref()).await
        } else {
            // 普通下载
            match timeout(
                timeout_duration,
                client.get(url).header("Range", "bytes=0-").send(),
            )
            .await
            {
                Ok(result) => match result {
                    Ok(response) => {
                        if !response.status().is_success() {
                            Err(anyhow!("HTTP错误: {}, URL: {}", response.status(), url))
                        } else {
                            let bytes = response.bytes().await?;
                            Ok(bytes.to_vec())
                        }
                    }
                    Err(e) => Err(anyhow!("请求失败: {}", e)),
                },
                Err(_) => Err(anyhow!("下载超时")),
            }
        };

        match download_result {
            Ok(data) => {
                let elapsed = start_time.elapsed();
                let size_kb = data.len() as f64 / 1024.0;
                let mbps = if elapsed.as_millis() > 0 {
                    (data.len() as f64 * 8.0) / (elapsed.as_millis() as f64) * 1000.0
                        / (1024.0 * 1024.0)
                } else {
                    0.0
                };
                trace!(
                    "下载成功: {:.2} KB / {:.2}s / {:.2} Mbps",
                    size_kb,
                    elapsed.as_secs_f64(),
                    mbps
                );
                return Ok(data);
            }
            Err(e) => {
                last_error = Some(e.to_string());

                // 根据错误类型决定重试策略
                let should_retry = if e.to_string().contains("timeout")
                    || e.to_string().contains("reset")
                    || e.to_string().contains("broken pipe")
                    || e.to_string().contains("connection")
                {
                    // 网络问题，立即重试
                    backoff_interval = retry_interval.min(1);
                    true
                } else if e.to_string().contains("500")
                    || e.to_string().contains("503")
                    || e.to_string().contains("502")
                    || e.to_string().contains("504")
                {
                    // 服务器错误，可能是临时的，使用退避重试
                    backoff_interval = (backoff_interval * 15) / 10; // 增加50%
                    true
                } else if e.to_string().contains("429") {
                    // 速率限制，需要更长时间等待
                    backoff_interval *= 2;
                    true
                } else if e.to_string().contains("404") || e.to_string().contains("401") {
                    // 客户端错误，不太可能通过重试解决
                    false
                } else if e.to_string().contains("403") {
                    // 403错误可能是临时的访问限制，使用更长的退避时间
                    backoff_interval *= 3;
                    true
                } else {
                    // 其他错误，使用标准退避
                    true
                };

                if should_retry && retry_count < retries {
                    retry_count += 1;

                    // 应用抖动避免雪崩
                    let jitter = rand::random::<u64>() % (backoff_interval * 100) / 100; // 添加0-100%的随机抖动
                    let retry_delay = Duration::from_secs(backoff_interval + jitter);

                    warn!(
                        "第 {} 次重试, 将在 {}s 后重试, 错误: {}, URL: {}",
                        retry_count,
                        retry_delay.as_secs(),
                        e,
                        url
                    );
                    sleep(retry_delay).await;
                } else {
                    break;
                }
            }
        }
    }

    Err(anyhow!(
        "下载失败 (重试 {} 次): {}, URL: {}",
        retry_count,
        last_error.unwrap_or_else(|| "未知错误".to_string()),
        url
    ))
}

/// 流式下载，用于大文件和实时处理
async fn stream_download(
    client: &Client,
    url: &str,
    timeout_duration: Duration,
    chunk_callback: Option<&Box<dyn Fn(usize, usize, &[u8]) + Send + Sync>>,
) -> Result<Vec<u8>> {
    let response = timeout(
        timeout_duration,
        client.get(url).header("Accept-Encoding", "identity").send(),
    )
    .await
    .context("请求超时")??;

    if !response.status().is_success() {
        return Err(anyhow!("HTTP错误: {}, URL: {}", response.status(), url));
    }

    let content_length = response.content_length().unwrap_or(0);

    let mut downloaded_size = 0;
    let mut buffer = Vec::new();

    // 设置一个合理的初始容量
    if content_length > 0 {
        buffer.reserve(content_length as usize);
    } else {
        buffer.reserve(64 * 1024); // 默认64KB
    }

    let mut stream = response.bytes_stream();
    let mut interrupted = false;

    loop {
        match timeout(timeout_duration, stream.next()).await {
            Ok(Some(chunk_result)) => {
                let chunk = chunk_result?;
                let chunk_size = chunk.len();

                // 调用回调函数处理数据块
                if let Some(callback) = chunk_callback {
                    callback(downloaded_size, chunk_size, &chunk);
                }

                buffer.extend_from_slice(&chunk);
                downloaded_size += chunk_size;

                // 每5MB打印一次进度
                if content_length > 0 && downloaded_size % (5 * 1024 * 1024) < chunk_size {
                    let percent = (downloaded_size as f64 / content_length as f64) * 100.0;
                    trace!(
                        "下载进度: {:.1}% ({}/{} 字节)",
                        percent,
                        downloaded_size,
                        content_length
                    );
                }
            }
            Ok(None) => break, // 流正常结束
            Err(_) => {
                // 读取超时：不能静默返回截断的数据，必须报错以触发重试
                interrupted = true;
                break;
            }
        }
    }

    // 数据不完整时直接报错（触发上层重试），绝不返回截断的数据
    // 消息附带 timeout/connection 英文关键词，命中 download_with_retry 的快速重试分类
    if interrupted || (content_length > 0 && downloaded_size != content_length as usize) {
        let reason = if interrupted {
            "读取超时 (read timeout)"
        } else {
            "连接提前关闭 (connection closed)"
        };
        return Err(anyhow!(
            "下载不完整 ({}): 预期 {} 字节，实际 {} 字节, URL: {}",
            reason,
            content_length,
            downloaded_size,
            url
        ));
    }

    trace!("下载完成: {} 字节", downloaded_size);
    Ok(buffer)
}
