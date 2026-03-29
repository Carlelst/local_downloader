/// 领域实体定义
/// 包含核心业务对象和其行为
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// 配置实体 - 系统配置的核心实体
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub m3u8_urls: Vec<String>,
    pub max_concurrent_downloads: usize,
    pub download_retries: u8,
    pub segment_timeout: u64,
    pub retry_interval: u64,
    pub output_directory: String,
    pub log_level: String,
    pub video_format: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub buffer_size: usize,
    pub validate_segments: bool,
    pub use_hw_accel: bool,
    pub thread_count: i32,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub chunk_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            m3u8_urls: Vec::new(),
            max_concurrent_downloads: 8,
            download_retries: 3,
            segment_timeout: 30,
            retry_interval: 2,
            output_directory: ".".to_string(),
            log_level: "info".to_string(),
            video_format: "mp4".to_string(),
            video_codec: "copy".to_string(),
            audio_codec: "aac".to_string(),
            buffer_size: 4 * 1024 * 1024, // 4MB
            validate_segments: false,
            use_hw_accel: true,
            thread_count: 0, // 自动选择
            video_bitrate: None,
            audio_bitrate: None,
            chunk_size: 1024 * 1024, // 1MB
        }
    }
}

/// 播放列表实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub url: String,
    pub segments: Vec<Segment>,
    pub encryption: Option<EncryptionInfo>,
    pub metadata: PlaylistMetadata,
    pub created_at: SystemTime,
}

/// 片段实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub index: usize,
    pub url: String,
    pub duration: f64,
    pub size: Option<u64>,
    pub encryption: Option<EncryptionInfo>,
    pub metadata: SegmentMetadata,
}

/// 加密信息实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    pub method: EncryptionMethod,
    pub key_url: Option<String>,
    pub key_data: Option<Vec<u8>>,
    pub iv: Option<Vec<u8>>,
}

/// 加密方法枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EncryptionMethod {
    None,
    Aes128,
    Aes256,
    SampleAes,
}

/// 播放列表元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistMetadata {
    pub version: Option<u32>,
    pub target_duration: Option<f64>,
    pub media_sequence: Option<u64>,
    pub playlist_type: Option<String>,
    pub is_live: bool,
}

/// 片段元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMetadata {
    pub byte_range: Option<(u64, u64)>,
    pub discontinuity: bool,
    pub program_date_time: Option<SystemTime>,
    pub custom_attributes: HashMap<String, String>,
}

/// 下载统计实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStats {
    pub total_segments: usize,
    pub completed_segments: usize,
    pub failed_segments: usize,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub download_speed: f64, // bytes per second
    pub estimated_time_remaining: Option<Duration>,
    pub start_time: SystemTime,
    pub last_update: SystemTime,
}

/// 处理结果实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub task_id: String,
    pub output_file: String,
    pub success: bool,
    pub error: Option<String>,
    pub stats: DownloadStats,
    pub processing_time: Duration,
    pub created_at: SystemTime,
}

/// 性能指标实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub task_id: String,
    pub cpu_usage: f64,
    pub memory_usage: u64,
    pub network_throughput: f64,
    pub disk_io: f64,
    pub concurrent_downloads: usize,
    pub error_rate: f64,
    pub timestamp: SystemTime,
}

impl Default for PlaylistMetadata {
    fn default() -> Self {
        Self {
            version: None,
            target_duration: None,
            media_sequence: None,
            playlist_type: None,
            is_live: false,
        }
    }
}

impl Default for SegmentMetadata {
    fn default() -> Self {
        Self {
            byte_range: None,
            discontinuity: false,
            program_date_time: None,
            custom_attributes: HashMap::new(),
        }
    }
}

impl Default for DownloadStats {
    fn default() -> Self {
        Self::new(0)
    }
}

impl DownloadStats {
    pub fn new(total_segments: usize) -> Self {
        let now = SystemTime::now();
        Self {
            total_segments,
            completed_segments: 0,
            failed_segments: 0,
            total_bytes: 0,
            downloaded_bytes: 0,
            download_speed: 0.0,
            estimated_time_remaining: None,
            start_time: now,
            last_update: now,
        }
    }

    pub fn update_progress(&mut self, completed: usize, downloaded_bytes: u64) {
        self.completed_segments = completed;
        self.downloaded_bytes = downloaded_bytes;
        self.last_update = SystemTime::now();

        // 计算下载速度
        if let Ok(elapsed) = self.last_update.duration_since(self.start_time) {
            if elapsed.as_secs() > 0 {
                self.download_speed = self.downloaded_bytes as f64 / elapsed.as_secs_f64();
            }
        }

        // 估算剩余时间
        if self.download_speed > 0.0 && self.total_bytes > 0 {
            let remaining_bytes = self.total_bytes.saturating_sub(self.downloaded_bytes);
            let remaining_seconds = remaining_bytes as f64 / self.download_speed;
            self.estimated_time_remaining = Some(Duration::from_secs_f64(remaining_seconds));
        }
    }

    pub fn completion_percentage(&self) -> f64 {
        if self.total_segments == 0 {
            0.0
        } else {
            (self.completed_segments as f64 / self.total_segments as f64) * 100.0
        }
    }
}
