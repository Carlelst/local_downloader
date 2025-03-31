/// 模块声明和公共 API 定义
pub mod decryptor;
pub mod downloader;
pub mod ffmpeg;
pub mod m3u8_parser;
pub mod segment_processor;

// 重新导出模块中的公共函数，方便外部调用
pub use decryptor::decrypt_segment;
pub use downloader::download_with_retry;
pub use ffmpeg::start_ffmpeg;
pub use m3u8_parser::parse_m3u8;
pub use segment_processor::process_segment;

// 外部 crate 声明
extern crate anyhow;
extern crate log;

use anyhow::{Context, Result};
use log::LevelFilter;
use serde::Deserialize;
use serde_yaml::from_reader;
use std::fs::File;

/// 配置结构体定义，用于加载 YAML 配置文件
#[derive(Debug, Deserialize)]
pub struct Config {
    pub m3u8_urls: Vec<String>,
    pub max_concurrent_downloads: usize,
    pub download_retries: u8,
    pub segment_timeout: u64,
    pub retry_interval: u64,
    #[serde(default = "default_output_directory")]
    pub output_directory: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_video_format")]
    pub video_format: String,
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,
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

/// 加载 YAML 配置文件并解析
pub fn load_config() -> Result<Config> {
    let file = File::open("config.yaml").context("无法打开配置文件")?;
    let config: Config = from_reader(file).context("解析配置文件失败")?;
    Ok(config)
}

/// 将字符串日志级别转换为 LevelFilter
pub fn parse_log_level(level: &str) -> LevelFilter {
    match level.to_lowercase().as_str() {
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}
