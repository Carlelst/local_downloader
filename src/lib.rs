//! # Video Processor Library
//!
//! A high-performance Rust library for downloading and processing M3U8 video streams.
//!
//! ## Features
//!
//! - **Concurrent Downloads**: Adaptive concurrent downloading with bandwidth monitoring
//! - **AES Decryption**: Support for multiple encryption algorithms and key formats
//! - **Hardware Acceleration**: Automatic detection and use of hardware-accelerated encoding
//! - **Error Recovery**: Intelligent retry mechanisms and error classification
//! - **Progress Monitoring**: Real-time progress tracking and performance metrics
//! - **Configuration Management**: Multi-source configuration with hot reloading
//! - **Structured Logging**: Comprehensive logging with context information
//!
//! ## Architecture
//!
//! The library follows a layered architecture pattern:
//!
//! - **Presentation Layer**: CLI interfaces and configuration management
//! - **Application Layer**: Business logic coordination and workflow management  
//! - **Domain Layer**: Core business logic and entities
//! - **Infrastructure Layer**: External service integration and data persistence
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use video_processor::{load_config, create_default_service_factory};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration
//!     let config = load_config()?;
//!     
//!     // Create service factory
//!     let factory = create_default_service_factory().await?;
//!     
//!     // Get video processor service
//!     let processor = factory.video_processor_service().await?;
//!     
//!     // Process M3U8 URLs
//!     for url in &config.m3u8_urls {
//!         let result = processor.process_stream(url, &config.output_directory).await?;
//!         println!("Processed: {:?}", result);
//!     }
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration
//!
//! Create a `config.yaml` file:
//!
//! ```yaml
//! m3u8_urls:
//!   - "https://example.com/playlist.m3u8"
//! max_concurrent_downloads: 10
//! download_retries: 3
//! segment_timeout: 30
//! output_directory: "./downloads"
//! log_level: "info"
//! video_format: "mp4"
//! use_hw_accel: true
//! ```

/// 模块声明和公共 API 定义

/// Presentation layer - handles user interfaces and external interactions
pub mod presentation;

/// Application layer - business logic coordination and workflow management
pub mod application;

/// Domain layer - core business logic and entities
pub mod domain;

/// Infrastructure layer - external service integration and data persistence
pub mod infrastructure;

/// Dependency injection container for managing service lifecycles
pub mod di;

/// Compatibility layer for backward compatibility with existing code
pub mod compatibility;

/// Legacy modules for backward compatibility
///
/// These modules provide the original API for existing code that hasn't been
/// migrated to the new architecture yet.
pub mod decryptor;
pub mod downloader;
pub mod ffmpeg;
pub mod m3u8_parser;
pub mod segment_processor;

// 重新导出模块中的公共函数，方便外部调用
pub use decryptor::{
    decrypt_segment, decrypt_segment_with_info, decrypt_segment_with_method, detect_key_format,
    validate_encryption_params,
};
pub use downloader::download_with_retry;
pub use ffmpeg::start_ffmpeg;
pub use infrastructure::DefaultM3U8Parser;
pub use m3u8_parser::parse_m3u8;
pub use segment_processor::process_segment;

// 重新导出兼容性模块
pub use compatibility::{
    batch_download_urls, batch_process_segments, create_compatible_download_manager,
    create_compatible_ffmpeg_handler, create_compatible_segment_processor,
    detect_hardware_acceleration, download_with_retry_new, optimize_encoding_parameters,
    process_segment_new, start_ffmpeg_new, FFmpegProcessMonitor,
};

// 重新导出新架构的核心类型
pub use application::{
    create_default_service_factory, create_legacy_adapter, ConfigService, LegacyServiceAdapter,
    MonitoringService, ServiceFactory, VideoProcessorService,
};
pub use domain::{Config as DomainConfig, DownloadStats, PerformanceMetrics, ProcessingResult};
pub use infrastructure::{
    create_app_logger_config, create_context, init_console_logger, init_file_logger,
    init_hybrid_logger, parse_log_level_string, LogEntry, LogFormat, LogLevel, LogOutput,
    LogRotationConfig, LoggerConfig, SimpleLogger,
};
pub use presentation::{CliInterface, ConfigPresentation, EnhancedCli, EnhancedCliConfig};

// 外部 crate 声明
extern crate anyhow;
extern crate log;

use anyhow::{Context, Result};
use log::LevelFilter;
use serde::Deserialize;
use serde_yaml::from_reader;
use std::fs::File;

/// Configuration structure for loading YAML configuration files
///
/// This structure defines all the configuration options available for the video processor.
/// It supports loading from YAML files with sensible defaults for all optional fields.
///
/// # Examples
///
/// ```rust,no_run
/// use video_processor::{Config, load_config};
///
/// // Load from config.yaml file
/// let config = load_config().expect("Failed to load configuration");
///
/// println!("Max concurrent downloads: {}", config.max_concurrent_downloads);
/// println!("Output directory: {}", config.output_directory);
/// ```
#[derive(Debug, Deserialize)]
pub struct Config {
    /// List of M3U8 URLs to process
    pub m3u8_urls: Vec<String>,
    /// Maximum number of concurrent downloads (default: varies by system)
    pub max_concurrent_downloads: usize,
    /// Number of retry attempts for failed downloads (default: 3)
    pub download_retries: u8,
    /// Timeout for individual segment downloads in seconds (default: 30)
    pub segment_timeout: u64,
    /// Interval between retry attempts in seconds (default: 1)
    pub retry_interval: u64,
    /// Output directory for processed videos (default: ".")
    #[serde(default = "default_output_directory")]
    pub output_directory: String,
    /// Log level: debug, info, warn, error (default: "info")
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Output video format (default: "mp4")
    #[serde(default = "default_video_format")]
    pub video_format: String,
    /// Video codec to use (default: "copy")
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    /// Audio codec to use (default: "aac")
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    /// Buffer size for I/O operations in bytes (default: 4MB)
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,
    /// Whether to validate segment accessibility before processing (default: false)
    #[serde(default = "default_validate_segments")]
    pub validate_segments: bool,
    /// Whether to use hardware acceleration when available (default: true)
    #[serde(default = "default_use_hw_accel")]
    pub use_hw_accel: bool,
    /// Number of threads for processing (0 = auto) (default: 0)
    #[serde(default = "default_thread_count")]
    pub thread_count: i32,
    /// Video bitrate (optional, e.g., "2M")
    #[serde(default)]
    pub video_bitrate: Option<String>,
    /// Audio bitrate (optional, e.g., "128k")
    #[serde(default)]
    pub audio_bitrate: Option<String>,
    /// Chunk size for downloads in bytes (default: 1MB)
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
}

fn default_output_directory() -> String {
    ".".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_video_format() -> String {
    "mp4".to_string()
}

fn default_video_codec() -> String {
    "copy".to_string()
}

fn default_audio_codec() -> String {
    "aac".to_string()
}

fn default_buffer_size() -> usize {
    1024 * 1024 * 4 // 默认缓冲区大小为4MB
}

fn default_validate_segments() -> bool {
    false // 默认不验证片段可访问性
}

fn default_use_hw_accel() -> bool {
    true // 默认启用硬件加速
}

fn default_thread_count() -> i32 {
    0 // 自动选择线程数
}

fn default_chunk_size() -> usize {
    1024 * 1024 // 默认1MB块大小
}

/// Load YAML configuration file and parse it
///
/// This function loads the configuration from a `config.yaml` file in the current directory.
/// All configuration fields have sensible defaults, so only the required fields need to be
/// specified in the YAML file.
///
/// # Returns
///
/// Returns a `Result<Config>` containing the parsed configuration or an error if the file
/// cannot be read or parsed.
///
/// # Errors
///
/// This function will return an error if:
/// - The `config.yaml` file cannot be opened
/// - The YAML content is malformed or invalid
/// - Required fields are missing from the configuration
///
/// # Examples
///
/// ```rust,no_run
/// use video_processor::load_config;
///
/// match load_config() {
///     Ok(config) => {
///         println!("Loaded {} URLs", config.m3u8_urls.len());
///         // Use the configuration...
///     },
///     Err(e) => {
///         eprintln!("Failed to load configuration: {}", e);
///     }
/// }
/// ```
pub fn load_config() -> Result<Config> {
    let file = File::open("config.yaml").context("无法打开配置文件")?;
    let config: Config = from_reader(file).context("解析配置文件失败")?;
    Ok(config)
}

/// Convert string log level to LevelFilter
///
/// This function converts a string representation of a log level to the corresponding
/// `LevelFilter` enum value. It's case-insensitive and defaults to `Info` for unknown values.
///
/// # Arguments
///
/// * `level` - A string slice representing the log level ("debug", "info", "warn", "error")
///
/// # Returns
///
/// Returns the corresponding `LevelFilter` value.
///
/// # Examples
///
/// ```rust
/// use video_processor::parse_log_level;
/// use log::LevelFilter;
///
/// assert_eq!(parse_log_level("debug"), LevelFilter::Debug);
/// assert_eq!(parse_log_level("INFO"), LevelFilter::Info);
/// assert_eq!(parse_log_level("unknown"), LevelFilter::Info); // defaults to Info
/// ```
pub fn parse_log_level(level: &str) -> LevelFilter {
    match level.to_lowercase().as_str() {
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}
