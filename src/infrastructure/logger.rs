/// 简单的日志系统实现
///
/// 提供结构化日志记录、日志级别控制和基本的日志轮转功能
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 日志级别枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::Error => LogLevel::Error,
            Level::Warn => LogLevel::Warn,
            Level::Info => LogLevel::Info,
            Level::Debug => LogLevel::Debug,
            Level::Trace => LogLevel::Trace,
        }
    }
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Level::Error,
            LogLevel::Warn => Level::Warn,
            LogLevel::Info => Level::Info,
            LogLevel::Debug => Level::Debug,
            LogLevel::Trace => Level::Trace,
        }
    }
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}

/// 结构化日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub module: String,
    pub message: String,
    pub context: HashMap<String, Value>,
    pub trace_id: Option<String>,
}

impl LogEntry {
    /// 创建新的日志条目
    pub fn new(
        level: LogLevel,
        module: String,
        message: String,
        context: HashMap<String, Value>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            module,
            message,
            context,
            trace_id,
        }
    }

    /// 将日志条目序列化为JSON字符串
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Failed to serialize log entry to JSON")
    }

    /// 将日志条目格式化为人类可读的字符串
    pub fn to_formatted_string(&self) -> String {
        let context_str = if self.context.is_empty() {
            String::new()
        } else {
            format!(
                " [{}]",
                self.context
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let trace_str = if let Some(ref trace_id) = self.trace_id {
            format!(" [trace:{}]", trace_id)
        } else {
            String::new()
        };

        format!(
            "{} [{:5}] [{}] {}{}{}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S%.3f UTC"),
            format!("{:?}", self.level).to_uppercase(),
            self.module,
            self.message,
            context_str,
            trace_str
        )
    }
}

/// 日志轮转配置
#[derive(Debug, Clone)]
pub struct LogRotationConfig {
    /// 最大文件大小（字节）
    pub max_file_size: u64,
    /// 保留的日志文件数量
    pub max_files: usize,
    /// 是否启用轮转
    pub enabled: bool,
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_files: 5,
            enabled: true,
        }
    }
}

/// 日志输出目标
#[derive(Debug, Clone)]
pub enum LogOutput {
    /// 输出到标准输出
    Stdout,
    /// 输出到标准错误
    Stderr,
    /// 输出到文件
    File(PathBuf),
    /// 同时输出到多个目标
    Multiple(Vec<LogOutput>),
}

/// 日志配置
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    /// 日志级别
    pub level: LogLevel,
    /// 输出目标
    pub output: LogOutput,
    /// 日志格式（JSON或格式化文本）
    pub format: LogFormat,
    /// 轮转配置
    pub rotation: LogRotationConfig,
    /// 是否包含模块路径
    pub include_module_path: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            output: LogOutput::Stdout,
            format: LogFormat::Formatted,
            rotation: LogRotationConfig::default(),
            include_module_path: true,
        }
    }
}

/// 日志格式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogFormat {
    /// JSON格式
    Json,
    /// 格式化文本
    Formatted,
}

/// 简单的日志记录器实现
pub struct SimpleLogger {
    config: LoggerConfig,
    writers: Arc<Mutex<HashMap<String, BufWriter<File>>>>,
}

impl SimpleLogger {
    /// 创建新的日志记录器
    pub fn new(config: LoggerConfig) -> Result<Self> {
        let logger = Self {
            config,
            writers: Arc::new(Mutex::new(HashMap::new())),
        };

        // 如果输出到文件，确保目录存在
        if let LogOutput::File(ref path) = logger.config.output {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create log directory: {:?}", parent))?;
            }
        }

        Ok(logger)
    }

    /// 初始化全局日志记录器
    pub fn init(config: LoggerConfig) -> Result<()> {
        let logger = Self::new(config)?;
        let level_filter: LevelFilter = logger.config.level.into();

        log::set_boxed_logger(Box::new(logger))
            .map_err(|e| anyhow::anyhow!("Failed to set logger: {}", e))?;
        log::set_max_level(level_filter);

        Ok(())
    }

    /// 写入日志条目
    fn write_log_entry(&self, entry: &LogEntry) -> Result<()> {
        let log_line = match self.config.format {
            LogFormat::Json => entry.to_json()?,
            LogFormat::Formatted => entry.to_formatted_string(),
        };

        match &self.config.output {
            LogOutput::Stdout => {
                println!("{}", log_line);
            }
            LogOutput::Stderr => {
                eprintln!("{}", log_line);
            }
            LogOutput::File(path) => {
                self.write_to_file(path, &log_line)?;
            }
            LogOutput::Multiple(outputs) => {
                for output in outputs {
                    let temp_config = LoggerConfig {
                        output: output.clone(),
                        ..self.config.clone()
                    };
                    let temp_logger = Self {
                        config: temp_config,
                        writers: self.writers.clone(),
                    };
                    temp_logger.write_log_entry(entry)?;
                }
            }
        }

        Ok(())
    }

    /// 写入到文件
    fn write_to_file(&self, path: &Path, content: &str) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        // 检查是否需要轮转
        if self.config.rotation.enabled {
            self.rotate_if_needed(path)?;
        }

        let mut writers = self.writers.lock().unwrap();

        // 获取或创建文件写入器
        if !writers.contains_key(&path_str) {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("Failed to open log file: {:?}", path))?;
            writers.insert(path_str.clone(), BufWriter::new(file));
        }

        if let Some(writer) = writers.get_mut(&path_str) {
            writeln!(writer, "{}", content)
                .with_context(|| format!("Failed to write to log file: {:?}", path))?;
            writer
                .flush()
                .with_context(|| format!("Failed to flush log file: {:?}", path))?;
        }

        Ok(())
    }

    /// 检查并执行日志轮转
    fn rotate_if_needed(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to get metadata for log file: {:?}", path))?;

        if metadata.len() >= self.config.rotation.max_file_size {
            self.rotate_logs(path)?;
        }

        Ok(())
    }

    /// 执行日志轮转
    fn rotate_logs(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        // 关闭当前文件写入器
        {
            let mut writers = self.writers.lock().unwrap();
            writers.remove(&path_str);
        }

        // 轮转现有文件
        for i in (1..self.config.rotation.max_files).rev() {
            let old_path = if i == 1 {
                path.to_path_buf()
            } else {
                path.with_extension(format!(
                    "{}.{}",
                    path.extension().and_then(|s| s.to_str()).unwrap_or("log"),
                    i - 1
                ))
            };

            let new_path = path.with_extension(format!(
                "{}.{}",
                path.extension().and_then(|s| s.to_str()).unwrap_or("log"),
                i
            ));

            if old_path.exists() {
                if i == self.config.rotation.max_files {
                    // 删除最老的文件
                    std::fs::remove_file(&old_path).with_context(|| {
                        format!("Failed to remove old log file: {:?}", old_path)
                    })?;
                } else {
                    // 重命名文件
                    std::fs::rename(&old_path, &new_path).with_context(|| {
                        format!(
                            "Failed to rotate log file from {:?} to {:?}",
                            old_path, new_path
                        )
                    })?;
                }
            }
        }

        Ok(())
    }

    /// 记录日志
    pub fn log_entry(&self, entry: LogEntry) -> Result<()> {
        self.write_log_entry(&entry)
    }

    /// 便捷方法：记录错误日志
    pub fn error(
        &self,
        module: &str,
        message: &str,
        context: HashMap<String, Value>,
    ) -> Result<()> {
        let entry = LogEntry::new(
            LogLevel::Error,
            module.to_string(),
            message.to_string(),
            context,
            None,
        );
        self.log_entry(entry)
    }

    /// 便捷方法：记录警告日志
    pub fn warn(&self, module: &str, message: &str, context: HashMap<String, Value>) -> Result<()> {
        let entry = LogEntry::new(
            LogLevel::Warn,
            module.to_string(),
            message.to_string(),
            context,
            None,
        );
        self.log_entry(entry)
    }

    /// 便捷方法：记录信息日志
    pub fn info(&self, module: &str, message: &str, context: HashMap<String, Value>) -> Result<()> {
        let entry = LogEntry::new(
            LogLevel::Info,
            module.to_string(),
            message.to_string(),
            context,
            None,
        );
        self.log_entry(entry)
    }

    /// 便捷方法：记录调试日志
    pub fn debug(
        &self,
        module: &str,
        message: &str,
        context: HashMap<String, Value>,
    ) -> Result<()> {
        let entry = LogEntry::new(
            LogLevel::Debug,
            module.to_string(),
            message.to_string(),
            context,
            None,
        );
        self.log_entry(entry)
    }
}

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let level: Level = self.config.level.into();
        metadata.level() <= level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let module = if self.config.include_module_path {
                record.module_path().unwrap_or("unknown").to_string()
            } else {
                record.target().to_string()
            };

            let mut context = HashMap::new();
            if let Some(file) = record.file() {
                context.insert("file".to_string(), Value::String(file.to_string()));
            }
            if let Some(line) = record.line() {
                context.insert("line".to_string(), Value::Number(line.into()));
            }

            let entry = LogEntry::new(
                record.level().into(),
                module,
                record.args().to_string(),
                context,
                None,
            );

            if let Err(e) = self.write_log_entry(&entry) {
                eprintln!("Failed to write log entry: {}", e);
            }
        }
    }

    fn flush(&self) {
        // 刷新所有文件写入器
        if let Ok(mut writers) = self.writers.lock() {
            for writer in writers.values_mut() {
                let _ = writer.flush();
            }
        }
    }
}

/// 日志宏辅助函数
pub fn create_context() -> HashMap<String, Value> {
    HashMap::new()
}

/// 添加上下文信息的宏
#[macro_export]
macro_rules! log_context {
    ($($key:expr => $value:expr),* $(,)?) => {
        {
            let mut context = std::collections::HashMap::new();
            $(
                context.insert($key.to_string(), serde_json::json!($value));
            )*
            context
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_log_entry_creation() {
        let context = log_context! {
            "user_id" => 123,
            "action" => "login"
        };

        let entry = LogEntry::new(
            LogLevel::Info,
            "auth".to_string(),
            "User logged in successfully".to_string(),
            context,
            Some("trace-123".to_string()),
        );

        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.module, "auth");
        assert_eq!(entry.message, "User logged in successfully");
        assert_eq!(entry.trace_id, Some("trace-123".to_string()));
        assert_eq!(entry.context.len(), 2);
    }

    #[test]
    fn test_log_entry_json_serialization() {
        let context = log_context! {
            "test" => "value"
        };

        let entry = LogEntry::new(
            LogLevel::Error,
            "test_module".to_string(),
            "Test message".to_string(),
            context,
            None,
        );

        let json = entry.to_json().unwrap();
        assert!(json.contains("\"level\":\"Error\""));
        assert!(json.contains("\"module\":\"test_module\""));
        assert!(json.contains("\"message\":\"Test message\""));
    }

    #[test]
    fn test_log_entry_formatted_string() {
        let context = log_context! {
            "key1" => "value1",
            "key2" => 42
        };

        let entry = LogEntry::new(
            LogLevel::Warn,
            "test_module".to_string(),
            "Test warning".to_string(),
            context,
            Some("trace-456".to_string()),
        );

        let formatted = entry.to_formatted_string();
        assert!(formatted.contains("WARN"));
        assert!(formatted.contains("test_module"));
        assert!(formatted.contains("Test warning"));
        assert!(formatted.contains("trace:trace-456"));
    }

    #[test]
    fn test_logger_config_default() {
        let config = LoggerConfig::default();
        assert_eq!(config.level, LogLevel::Info);
        assert!(matches!(config.output, LogOutput::Stdout));
        assert_eq!(config.format, LogFormat::Formatted);
        assert!(config.include_module_path);
    }

    #[test]
    fn test_simple_logger_creation() {
        let config = LoggerConfig::default();
        let logger = SimpleLogger::new(config);
        assert!(logger.is_ok());
    }

    #[test]
    fn test_log_rotation_config() {
        let config = LogRotationConfig::default();
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert_eq!(config.max_files, 5);
        assert!(config.enabled);
    }

    #[test]
    fn test_file_logging() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let log_path = temp_dir.path().join("test.log");

        let config = LoggerConfig {
            output: LogOutput::File(log_path.clone()),
            format: LogFormat::Formatted,
            ..LoggerConfig::default()
        };

        let logger = SimpleLogger::new(config)?;

        let context = log_context! {
            "test" => "file_logging"
        };

        logger.info("test_module", "Test file logging", context)?;

        // 验证文件是否创建并包含日志内容
        assert!(log_path.exists());
        let content = fs::read_to_string(&log_path)?;
        assert!(content.contains("Test file logging"));
        assert!(content.contains("test_module"));

        Ok(())
    }
}
