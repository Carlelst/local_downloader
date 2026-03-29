/// HTTP客户端基础设施
/// 提供HTTP请求的具体实现
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::infrastructure::AdaptiveCacheManager;

/// HTTP客户端trait
#[async_trait]
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// 发送GET请求
    async fn get(&self, url: &str, timeout: Option<Duration>) -> Result<HttpResponse>;

    /// 发送HEAD请求
    async fn head(&self, url: &str) -> Result<HttpResponse>;

    /// 发送带范围的GET请求（用于断点续传）
    async fn get_range(&self, url: &str, start: u64, end: Option<u64>) -> Result<Vec<u8>>;

    /// 流式下载
    async fn stream_download(
        &self,
        url: &str,
        chunk_callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> Result<Vec<u8>>;

    /// 带重试的请求
    async fn get_with_retry(&self, url: &str, max_retries: u32) -> Result<Vec<u8>>;
}

/// HTTP响应
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// 检查响应是否成功
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 获取状态码
    pub fn status_code(&self) -> u16 {
        self.status
    }

    /// 获取状态文本
    pub fn status_text(&self) -> Option<&str> {
        match self.status {
            200 => Some("OK"),
            404 => Some("Not Found"),
            500 => Some("Internal Server Error"),
            _ => None,
        }
    }

    /// 获取响应体作为文本
    pub async fn text(self) -> Result<String> {
        String::from_utf8(self.body).map_err(|e| anyhow::anyhow!("响应体不是有效的UTF-8: {}", e))
    }

    /// 获取响应体作为字节
    pub async fn bytes(self) -> Result<Vec<u8>> {
        Ok(self.body)
    }
}

/// HTTP错误类型
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("网络连接错误: {0}")]
    Connection(String),

    #[error("HTTP状态错误: {status} - {message}")]
    Status { status: u16, message: String },

    #[error("超时错误: {0}")]
    Timeout(String),

    #[error("重试次数超限: {attempts} 次尝试后失败")]
    RetryExhausted { attempts: u32 },

    #[error("请求错误: {0}")]
    Request(String),

    #[error("响应解析错误: {0}")]
    Parse(String),
}

/// 重试策略配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub jitter: bool,
}

/// 默认HTTP客户端实现
pub struct DefaultHttpClient {
    client: Client,
    timeout: Duration,
    retry_config: RetryConfig,
    cache_manager: Option<Arc<AdaptiveCacheManager>>,
    enable_caching: bool,
}

impl DefaultHttpClient {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| anyhow::anyhow!("创建HTTP客户端失败: {}", e))?;

        let retry_config = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        };

        Ok(Self {
            client,
            timeout: Duration::from_secs(timeout_secs),
            retry_config,
            cache_manager: None,
            enable_caching: false,
        })
    }

    pub fn with_cache(mut self, cache_manager: Arc<AdaptiveCacheManager>) -> Self {
        self.cache_manager = Some(cache_manager);
        self.enable_caching = true;
        self
    }

    pub fn with_client(client: Client) -> Self {
        let retry_config = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        };

        Self {
            client,
            timeout: Duration::from_secs(30),
            retry_config,
            cache_manager: None,
            enable_caching: false,
        }
    }

    pub fn with_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// 计算重试延迟时间
    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let base_delay_ms = self.retry_config.base_delay.as_millis() as f64;
        let delay_ms = base_delay_ms * self.retry_config.backoff_multiplier.powi(attempt as i32);

        let mut delay = Duration::from_millis(delay_ms as u64);

        // 限制最大延迟
        if delay > self.retry_config.max_delay {
            delay = self.retry_config.max_delay;
        }

        // 添加抖动
        if self.retry_config.jitter {
            use rand::Rng;
            let jitter_factor = rand::thread_rng().gen_range(0.5..1.5);
            delay = Duration::from_millis((delay.as_millis() as f64 * jitter_factor) as u64);
        }

        delay
    }

    /// 判断错误是否可重试
    fn is_retryable_error(&self, error: &reqwest::Error) -> bool {
        if error.is_timeout() || error.is_connect() {
            return true;
        }

        if let Some(status) = error.status() {
            // 5xx 服务器错误和部分 4xx 错误可重试
            matches!(status.as_u16(), 500..=599 | 408 | 429)
        } else {
            false
        }
    }

    /// 执行带重试的HTTP请求
    async fn execute_with_retry<F, Fut, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
    {
        let mut last_error_msg = None;
        let mut is_retryable = false;

        for attempt in 0..self.retry_config.max_attempts {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    let error_msg = error.to_string();
                    is_retryable = self.is_retryable_error(&error);
                    last_error_msg = Some(error_msg.clone());

                    if attempt < self.retry_config.max_attempts - 1 && is_retryable {
                        let delay = self.calculate_retry_delay(attempt);
                        log::warn!(
                            "请求失败，{}ms后重试 (尝试 {}/{}): {}",
                            delay.as_millis(),
                            attempt + 1,
                            self.retry_config.max_attempts,
                            error_msg
                        );
                        sleep(delay).await;
                    } else {
                        return Err(self.map_reqwest_error(error));
                    }
                }
            }
        }

        if is_retryable {
            Err(HttpError::RetryExhausted {
                attempts: self.retry_config.max_attempts,
            }
            .into())
        } else {
            Err(HttpError::Request(last_error_msg.unwrap_or_else(|| "未知错误".to_string())).into())
        }
    }

    /// 映射reqwest错误到自定义错误类型
    fn map_reqwest_error(&self, error: reqwest::Error) -> anyhow::Error {
        if error.is_timeout() {
            HttpError::Timeout(error.to_string()).into()
        } else if error.is_connect() {
            HttpError::Connection(error.to_string()).into()
        } else if let Some(status) = error.status() {
            HttpError::Status {
                status: status.as_u16(),
                message: error.to_string(),
            }
            .into()
        } else {
            HttpError::Request(error.to_string()).into()
        }
    }
}

#[async_trait]
impl HttpClient for DefaultHttpClient {
    async fn get(&self, url: &str, timeout: Option<Duration>) -> Result<HttpResponse> {
        // 检查缓存
        if self.enable_caching {
            if let Some(cache_manager) = &self.cache_manager {
                if let Some(cached_data) = cache_manager.get_cached_http_response(url).await {
                    log::debug!("缓存命中: {}", url);
                    return Ok(HttpResponse {
                        status: 200,
                        headers: HashMap::new(),
                        content_length: Some(cached_data.len() as u64),
                        supports_ranges: false,
                        body: cached_data,
                    });
                }
            }
        }

        let client = self.client.clone();
        let url_string = url.to_string();
        let timeout = timeout.unwrap_or(self.timeout);

        let response = self
            .execute_with_retry(|| async {
                let mut request = client.get(&url_string);
                if timeout != self.timeout {
                    request = request.timeout(timeout);
                }
                request.send().await
            })
            .await?;

        let status = response.status().as_u16();
        let content_length = response.content_length();

        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.to_string(), value_str.to_string());
            }
        }

        // 检查是否支持范围请求
        let supports_ranges = headers
            .get("accept-ranges")
            .map(|v| v.to_lowercase() == "bytes")
            .unwrap_or(false);

        let body = if response.status().is_success() {
            let body_bytes = response
                .bytes()
                .await
                .map_err(|e| self.map_reqwest_error(e))?
                .to_vec();

            // 缓存响应（如果启用缓存且响应成功）
            if self.enable_caching && status == 200 {
                if let Some(cache_manager) = &self.cache_manager {
                    if let Err(e) = cache_manager
                        .cache_http_response(url, body_bytes.clone())
                        .await
                    {
                        log::warn!("缓存HTTP响应失败: {}", e);
                    } else {
                        log::debug!("已缓存HTTP响应: {}", url);
                    }
                }
            }

            body_bytes
        } else {
            return Err(self.map_reqwest_error(response.error_for_status().unwrap_err()));
        };

        Ok(HttpResponse {
            status,
            headers,
            content_length,
            supports_ranges,
            body,
        })
    }

    async fn head(&self, url: &str) -> Result<HttpResponse> {
        let client = self.client.clone();
        let url = url.to_string();

        let response = self
            .execute_with_retry(|| async { client.head(&url).send().await })
            .await?;

        let status = response.status().as_u16();
        let content_length = response.content_length();

        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.to_string(), value_str.to_string());
            }
        }

        // 检查是否支持范围请求
        let supports_ranges = headers
            .get("accept-ranges")
            .map(|v| v.to_lowercase() == "bytes")
            .unwrap_or(false);

        Ok(HttpResponse {
            status,
            headers,
            content_length,
            supports_ranges,
            body: Vec::new(), // HEAD请求没有响应体
        })
    }

    async fn get_range(&self, url: &str, start: u64, end: Option<u64>) -> Result<Vec<u8>> {
        let client = self.client.clone();
        let url = url.to_string();

        let range_header = if let Some(end) = end {
            format!("bytes={}-{}", start, end)
        } else {
            format!("bytes={}-", start)
        };

        let result = self
            .execute_with_retry(|| async {
                let response = client
                    .get(&url)
                    .header("Range", &range_header)
                    .send()
                    .await?;

                // 206 Partial Content 或 200 OK 都可以接受
                if !matches!(response.status().as_u16(), 200 | 206) {
                    return Err(reqwest::Error::from(
                        response.error_for_status().unwrap_err(),
                    ));
                }

                response.bytes().await
            })
            .await?;

        Ok(result.to_vec())
    }

    async fn stream_download(
        &self,
        url: &str,
        chunk_callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> Result<Vec<u8>> {
        use futures_util::StreamExt;

        let client = self.client.clone();
        let url = url.to_string();

        let response = self
            .execute_with_retry(|| async { client.get(&url).send().await })
            .await?;

        if !response.status().is_success() {
            return Err(self.map_reqwest_error(response.error_for_status().unwrap_err()));
        }

        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| self.map_reqwest_error(e))?;
            let chunk_vec = chunk.to_vec();
            chunk_callback(chunk_vec.clone());
            buffer.extend_from_slice(&chunk_vec);
        }

        Ok(buffer)
    }

    async fn get_with_retry(&self, url: &str, max_retries: u32) -> Result<Vec<u8>> {
        let _original_config = self.retry_config.clone();
        let mut client = self.clone();
        client.retry_config.max_attempts = max_retries;

        let response = client.get(url, None).await?;
        Ok(response.body)
    }
}

impl Clone for DefaultHttpClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            timeout: self.timeout,
            retry_config: self.retry_config.clone(),
            cache_manager: self.cache_manager.clone(),
            enable_caching: self.enable_caching,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Mock HTTP客户端用于测试
#[cfg(test)]
pub struct MockHttpClient {
    responses: std::collections::HashMap<String, HttpResponse>,
}

#[cfg(test)]
impl MockHttpClient {
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
        }
    }

    pub fn with_response(mut self, url: &str, response: HttpResponse) -> Self {
        self.responses.insert(url.to_string(), response);
        self
    }

    pub fn with_text_response(mut self, url: &str, status: u16, text: &str) -> Self {
        let response = HttpResponse {
            status,
            headers: HashMap::new(),
            content_length: Some(text.len() as u64),
            supports_ranges: false,
            body: text.as_bytes().to_vec(),
        };
        self.responses.insert(url.to_string(), response);
        self
    }
}

#[cfg(test)]
#[async_trait]
impl HttpClient for MockHttpClient {
    async fn get(&self, url: &str, _timeout: Option<Duration>) -> Result<HttpResponse> {
        if let Some(response) = self.responses.get(url) {
            Ok(HttpResponse {
                status: response.status,
                headers: response.headers.clone(),
                content_length: response.content_length,
                supports_ranges: response.supports_ranges,
                body: response.body.clone(),
            })
        } else {
            Err(anyhow::anyhow!("Mock response not found for URL: {}", url))
        }
    }

    async fn head(&self, url: &str) -> Result<HttpResponse> {
        if let Some(response) = self.responses.get(url) {
            Ok(HttpResponse {
                status: response.status,
                headers: response.headers.clone(),
                content_length: response.content_length,
                supports_ranges: response.supports_ranges,
                body: Vec::new(), // HEAD请求没有响应体
            })
        } else {
            Err(anyhow::anyhow!("Mock response not found for URL: {}", url))
        }
    }

    async fn get_range(&self, _url: &str, _start: u64, _end: Option<u64>) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn stream_download(
        &self,
        _url: &str,
        _chunk_callback: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    ) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn get_with_retry(&self, url: &str, _max_retries: u32) -> Result<Vec<u8>> {
        let response = self.get(url, None).await?;
        Ok(response.body)
    }
}
