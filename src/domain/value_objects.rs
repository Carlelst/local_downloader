/// 值对象定义
/// 包含不可变的值类型和业务概念
use serde::{Deserialize, Serialize};
use std::fmt;

/// URL值对象 - 确保URL的有效性
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Url(String);

impl Url {
    pub fn new(url: String) -> Result<Self, UrlError> {
        if url.trim().is_empty() {
            return Err(UrlError::Empty);
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(UrlError::InvalidScheme);
        }

        // 基本URL验证
        match url::Url::parse(&url) {
            Ok(_) => Ok(Url(url)),
            Err(_) => Err(UrlError::InvalidFormat),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// URL错误类型
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UrlError {
    #[error("URL不能为空")]
    Empty,
    #[error("无效的URL协议，必须是http或https")]
    InvalidScheme,
    #[error("无效的URL格式")]
    InvalidFormat,
}

/// 文件大小值对象
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileSize(u64);

impl FileSize {
    pub fn new(bytes: u64) -> Self {
        FileSize(bytes)
    }

    pub fn bytes(&self) -> u64 {
        self.0
    }

    pub fn kilobytes(&self) -> f64 {
        self.0 as f64 / 1024.0
    }

    pub fn megabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0)
    }

    pub fn gigabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for FileSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 1024 {
            write!(f, "{} B", self.0)
        } else if self.0 < 1024 * 1024 {
            write!(f, "{:.2} KB", self.kilobytes())
        } else if self.0 < 1024 * 1024 * 1024 {
            write!(f, "{:.2} MB", self.megabytes())
        } else {
            write!(f, "{:.2} GB", self.gigabytes())
        }
    }
}

/// 比特率值对象
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Bitrate(u64); // bits per second

impl Bitrate {
    pub fn new(bps: u64) -> Self {
        Bitrate(bps)
    }

    pub fn from_kbps(kbps: u64) -> Self {
        Bitrate(kbps * 1000)
    }

    pub fn from_mbps(mbps: u64) -> Self {
        Bitrate(mbps * 1000 * 1000)
    }

    pub fn bps(&self) -> u64 {
        self.0
    }

    pub fn kbps(&self) -> f64 {
        self.0 as f64 / 1000.0
    }

    pub fn mbps(&self) -> f64 {
        self.0 as f64 / (1000.0 * 1000.0)
    }
}

impl fmt::Display for Bitrate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 1000 {
            write!(f, "{} bps", self.0)
        } else if self.0 < 1000 * 1000 {
            write!(f, "{:.2} Kbps", self.kbps())
        } else {
            write!(f, "{:.2} Mbps", self.mbps())
        }
    }
}

/// 任务ID值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new() -> Self {
        use rand::Rng;
        let id: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        TaskId(id)
    }

    pub fn from_string(id: String) -> Result<Self, TaskIdError> {
        if id.trim().is_empty() {
            return Err(TaskIdError::Empty);
        }

        if id.len() < 8 || id.len() > 64 {
            return Err(TaskIdError::InvalidLength);
        }

        if !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(TaskIdError::InvalidCharacters);
        }

        Ok(TaskId(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 任务ID错误类型
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TaskIdError {
    #[error("任务ID不能为空")]
    Empty,
    #[error("任务ID长度必须在8-64字符之间")]
    InvalidLength,
    #[error("任务ID只能包含字母、数字、连字符和下划线")]
    InvalidCharacters,
}

/// 时间间隔值对象
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimeInterval(std::time::Duration);

impl TimeInterval {
    pub fn new(duration: std::time::Duration) -> Self {
        TimeInterval(duration)
    }

    pub fn from_seconds(seconds: u64) -> Self {
        TimeInterval(std::time::Duration::from_secs(seconds))
    }

    pub fn from_millis(millis: u64) -> Self {
        TimeInterval(std::time::Duration::from_millis(millis))
    }

    pub fn duration(&self) -> std::time::Duration {
        self.0
    }

    pub fn as_secs(&self) -> u64 {
        self.0.as_secs()
    }

    pub fn as_millis(&self) -> u128 {
        self.0.as_millis()
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl fmt::Display for TimeInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        if secs < 60 {
            write!(f, "{}s", secs)
        } else if secs < 3600 {
            write!(f, "{}m{}s", secs / 60, secs % 60)
        } else {
            write!(f, "{}h{}m{}s", secs / 3600, (secs % 3600) / 60, secs % 60)
        }
    }
}

/// 重试配置值对象
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: TimeInterval,
    pub max_delay: TimeInterval,
    pub backoff_multiplier: f64,
    pub jitter: bool,
}

impl RetryConfig {
    pub fn new(max_attempts: u32, initial_delay: TimeInterval) -> Self {
        Self {
            max_attempts,
            initial_delay,
            max_delay: TimeInterval::from_seconds(300), // 5分钟最大延迟
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    pub fn with_max_delay(mut self, max_delay: TimeInterval) -> Self {
        self.max_delay = max_delay;
        self
    }

    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    pub fn calculate_delay(&self, attempt: u32) -> TimeInterval {
        if attempt == 0 {
            return self.initial_delay;
        }

        let base_delay = self.initial_delay.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32 - 1);

        let delay = if base_delay > self.max_delay.as_millis() as f64 {
            self.max_delay.as_millis() as u64
        } else {
            base_delay as u64
        };

        let final_delay = if self.jitter {
            // 添加±25%的随机抖动
            use rand::Rng;
            let jitter_factor = rand::thread_rng().gen_range(0.75..=1.25);
            ((delay as f64) * jitter_factor) as u64
        } else {
            delay
        };

        TimeInterval::from_millis(final_delay)
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::new(3, TimeInterval::from_seconds(1))
    }
}
