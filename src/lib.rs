/// 模块声明和公共 API 定义
pub mod decryptor;
pub mod downloader;
pub mod m3u8_parser;
pub mod ffmpeg;
pub mod segment_processor;

// 重新导出模块中的公共函数，方便外部调用
pub use downloader::download_with_retry;
pub use m3u8_parser::parse_m3u8;
pub use ffmpeg::start_ffmpeg;
pub use decryptor::decrypt_segment;
pub use segment_processor::process_segment;

// 外部 crate 声明
extern crate anyhow;
extern crate log;

use anyhow::{Result, Context};
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
}

/// 加载 YAML 配置文件并解析
pub fn load_config() -> Result<Config> {
    let file = File::open("config.yaml").context("无法打开配置文件")?;
    let config: Config = from_reader(file).context("解析配置文件失败")?;
    Ok(config)
}
