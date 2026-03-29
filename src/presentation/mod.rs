/// 表示层 - 处理用户界面和外部交互
///
/// 该层负责：
/// - CLI接口和命令行参数处理
/// - 用户输入验证和格式化
/// - 输出格式化和显示
/// - 外部API接口（如果有）
pub mod cli;
pub mod cli_enhanced;
pub mod config;

pub use cli::*;
pub use cli_enhanced::*;
pub use config::*;
