/// 模块声明和公共 API 定义
pub mod downloader;
pub mod m3u8_parser;
pub mod ffmpeg;
pub mod decryptor;

pub use downloader::{download_with_retry};
pub use m3u8_parser::{parse_m3u8};
pub use ffmpeg::{start_ffmpeg};
pub use decryptor::{decrypt_segment};

extern crate anyhow;
extern crate log;

#[macro_export]
macro_rules! example_macro {
    ($expression:expr) => {
        println!("[INFO]: The result is: {}", $expression);
    };
}