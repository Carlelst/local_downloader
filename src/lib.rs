/// 模块声明和公共 API 定义
pub mod downloader;
pub mod m3u8_parser;
pub mod ffmpeg;
pub mod decryptor;

// 重新导出以便于外部访问公共函数
pub use downloader::download_with_retry;
pub use m3u8_parser::parse_m3u8;
pub use ffmpeg::start_ffmpeg;
pub use decryptor::decrypt_segment;

// 引入外部 crate
extern crate anyhow;
extern crate log;

/// 示例宏定义，打印表达式结果
#[macro_export]
macro_rules! example_macro {
    ($expression:expr) => {
        println!("[INFO]: The result is: {}", $expression);
    };
}
