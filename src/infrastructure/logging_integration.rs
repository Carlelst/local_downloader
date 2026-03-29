/// 日志系统集成辅助函数
///
/// 提供便捷的方法来初始化和配置日志系统
use anyhow::Result;
use std::path::PathBuf;

use super::logger::{
    LogFormat, LogLevel, LogOutput, LogRotationConfig, LoggerConfig, SimpleLogger,
};

/// 初始化默认的控制台日志记录器
pub fn init_console_logger(level: LogLevel) -> Result<()> {
    let config = LoggerConfig {
        level,
        output: LogOutput::Stdout,
        format: LogFormat::Formatted,
        ..LoggerConfig::default()
    };

    SimpleLogger::init(config)
}

/// 初始化文件日志记录器
pub fn init_file_logger(level: LogLevel, log_file: PathBuf) -> Result<()> {
    let config = LoggerConfig {
        level,
        output: LogOutput::File(log_file),
        format: LogFormat::Json,
        rotation: LogRotationConfig {
            max_file_size: 50 * 1024 * 1024, // 50MB
            max_files: 10,
            enabled: true,
        },
        ..LoggerConfig::default()
    };

    SimpleLogger::init(config)
}

/// 初始化混合日志记录器（同时输出到控制台和文件）
pub fn init_hybrid_logger(level: LogLevel, log_file: PathBuf) -> Result<()> {
    let config = LoggerConfig {
        level,
        output: LogOutput::Multiple(vec![LogOutput::Stdout, LogOutput::File(log_file)]),
        format: LogFormat::Formatted,
        rotation: LogRotationConfig {
            max_file_size: 20 * 1024 * 1024, // 20MB
            max_files: 5,
            enabled: true,
        },
        ..LoggerConfig::default()
    };

    SimpleLogger::init(config)
}

/// 从字符串解析日志级别
pub fn parse_log_level_string(level_str: &str) -> LogLevel {
    match level_str.to_lowercase().as_str() {
        "error" => LogLevel::Error,
        "warn" | "warning" => LogLevel::Warn,
        "info" => LogLevel::Info,
        "debug" => LogLevel::Debug,
        "trace" => LogLevel::Trace,
        _ => LogLevel::Info, // 默认级别
    }
}

/// 创建带有应用程序信息的日志配置
pub fn create_app_logger_config(
    app_name: &str,
    log_dir: Option<PathBuf>,
    level: LogLevel,
) -> Result<LoggerConfig> {
    let output = if let Some(dir) = log_dir {
        // 确保日志目录存在
        std::fs::create_dir_all(&dir)?;
        let log_file = dir.join(format!("{}.log", app_name));
        LogOutput::File(log_file)
    } else {
        LogOutput::Stdout
    };

    Ok(LoggerConfig {
        level,
        output,
        format: LogFormat::Json,
        rotation: LogRotationConfig {
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_files: 7,                     // 保留一周的日志
            enabled: true,
        },
        include_module_path: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_log_level_string() {
        assert_eq!(parse_log_level_string("error"), LogLevel::Error);
        assert_eq!(parse_log_level_string("ERROR"), LogLevel::Error);
        assert_eq!(parse_log_level_string("warn"), LogLevel::Warn);
        assert_eq!(parse_log_level_string("warning"), LogLevel::Warn);
        assert_eq!(parse_log_level_string("info"), LogLevel::Info);
        assert_eq!(parse_log_level_string("debug"), LogLevel::Debug);
        assert_eq!(parse_log_level_string("trace"), LogLevel::Trace);
        assert_eq!(parse_log_level_string("unknown"), LogLevel::Info);
    }

    #[test]
    fn test_create_app_logger_config() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = create_app_logger_config(
            "test_app",
            Some(temp_dir.path().to_path_buf()),
            LogLevel::Debug,
        )?;

        assert_eq!(config.level, LogLevel::Debug);
        assert_eq!(config.format, LogFormat::Json);
        assert!(config.rotation.enabled);
        assert_eq!(config.rotation.max_files, 7);

        Ok(())
    }

    #[test]
    fn test_create_console_logger_config() -> Result<()> {
        let config = create_app_logger_config("test_app", None, LogLevel::Info)?;

        assert_eq!(config.level, LogLevel::Info);
        assert!(matches!(config.output, LogOutput::Stdout));

        Ok(())
    }
}
