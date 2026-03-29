/// 兼容性模块 - 提供新旧架构之间的桥梁
///
/// 该模块确保现有代码可以继续使用旧的API，
/// 同时在内部使用新的架构实现
pub mod download_compat;
pub mod ffmpeg_compat;
pub mod segment_compat;

pub use download_compat::*;
pub use ffmpeg_compat::*;
pub use segment_compat::*;
