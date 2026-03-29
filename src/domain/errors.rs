use serde::{Deserialize, Serialize};
/// 错误处理框架 - 错误分类体系和恢复策略
///
/// 该模块定义了系统中所有可能的错误类型，以及相应的恢复策略。
/// 支持错误传播、上下文保持和错误统计分析。
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// 主要错误类型枚举
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum VideoProcessorError {
    #[error("网络错误: {0}")]
    Network(#[from] NetworkError),

    #[error("解析错误: {0}")]
    Parsing(#[from] ParsingError),

    #[error("处理错误: {0}")]
    Processing(#[from] ProcessingError),

    #[error("配置错误: {0}")]
    Configuration(#[from] ConfigError),

    #[error("系统错误: {0}")]
    System(#[from] SystemError),

    #[error("资源错误: {0}")]
    Resource(#[from] ResourceError),
}

/// 网络相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum NetworkError {
    #[error("连接超时: {url}")]
    ConnectionTimeout { url: String },

    #[error("DNS解析失败: {domain}")]
    DnsResolutionFailed { domain: String },

    #[error("HTTP错误 {status}: {message}")]
    HttpError { status: u16, message: String },

    #[error("网络不可达: {details}")]
    NetworkUnreachable { details: String },

    #[error("SSL/TLS错误: {details}")]
    SslError { details: String },
}

/// 解析相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ParsingError {
    #[error("M3U8格式错误: 行 {line}, {message}")]
    InvalidM3U8Format { line: usize, message: String },

    #[error("编码错误: {encoding}, {details}")]
    EncodingError { encoding: String, details: String },

    #[error("结构错误: {component}, {reason}")]
    StructureError { component: String, reason: String },

    #[error("缺少必需字段: {field}")]
    MissingRequiredField { field: String },
}

/// 处理相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ProcessingError {
    #[error("FFmpeg进程崩溃: 退出代码 {exit_code}")]
    FFmpegCrash { exit_code: i32 },

    #[error("解密失败: {algorithm}, {reason}")]
    DecryptionFailed { algorithm: String, reason: String },

    #[error("编码失败: {codec}, {details}")]
    EncodingFailed { codec: String, details: String },

    #[error("片段处理失败: 索引 {index}, {error}")]
    SegmentProcessingFailed { index: usize, error: String },
}

/// 配置相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ConfigError {
    #[error("配置文件不存在: {path}")]
    ConfigFileNotFound { path: String },

    #[error("配置验证失败: {field}, {reason}")]
    ValidationFailed { field: String, reason: String },

    #[error("配置冲突: {source1} vs {source2}")]
    ConfigConflict { source1: String, source2: String },

    #[error("无效的配置值: {key} = {value}")]
    InvalidValue { key: String, value: String },
}

/// 系统相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum SystemError {
    #[error("内存不足: 需要 {required}MB, 可用 {available}MB")]
    OutOfMemory { required: u64, available: u64 },

    #[error("磁盘空间不足: 需要 {required}MB, 可用 {available}MB")]
    InsufficientDiskSpace { required: u64, available: u64 },

    #[error("文件系统错误: {operation}, {path}, {reason}")]
    FileSystemError {
        operation: String,
        path: String,
        reason: String,
    },

    #[error("权限错误: {resource}, {required_permission}")]
    PermissionDenied {
        resource: String,
        required_permission: String,
    },
}

/// 资源相关错误
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ResourceError {
    #[error("资源不可用: {resource}")]
    ResourceUnavailable { resource: String },

    #[error("资源耗尽: {resource}, 限制: {limit}")]
    ResourceExhausted { resource: String, limit: String },

    #[error("资源锁定: {resource}, 持有者: {holder}")]
    ResourceLocked { resource: String, holder: String },
}

/// 错误严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// 信息性错误，不影响功能
    Info,
    /// 警告，可能影响性能
    Warning,
    /// 错误，影响部分功能
    Error,
    /// 严重错误，影响核心功能
    Critical,
    /// 致命错误，系统无法继续运行
    Fatal,
}

/// 恢复策略类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    /// 立即重试
    ImmediateRetry,
    /// 延迟重试
    DelayedRetry { delay: Duration },
    /// 指数退避重试
    ExponentialBackoff {
        base_delay: Duration,
        max_retries: u32,
    },
    /// 降级处理
    Degrade,
    /// 跳过当前操作
    Skip,
    /// 切换到备用方案
    Fallback { alternative: String },
    /// 停止处理
    Stop,
    /// 需要人工干预
    ManualIntervention,
}

/// 错误上下文信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    /// 错误发生的组件
    pub component: String,
    /// 错误发生的操作
    pub operation: String,
    /// 相关的资源或数据
    pub resource: Option<String>,
    /// 用户ID或会话ID
    pub user_id: Option<String>,
    /// 请求ID或事务ID
    pub request_id: Option<String>,
    /// 额外的上下文数据
    pub metadata: HashMap<String, String>,
}

/// 带上下文的错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualError {
    /// 原始错误
    pub error: VideoProcessorError,
    /// 错误上下文
    pub context: ErrorContext,
    /// 错误发生时间
    pub timestamp: SystemTime,
    /// 错误严重程度
    pub severity: ErrorSeverity,
    /// 建议的恢复策略
    pub recovery_strategy: RecoveryStrategy,
}

impl ContextualError {
    /// 创建新的上下文错误
    pub fn new(
        error: VideoProcessorError,
        context: ErrorContext,
        severity: ErrorSeverity,
        recovery_strategy: RecoveryStrategy,
    ) -> Self {
        Self {
            error,
            context,
            timestamp: SystemTime::now(),
            severity,
            recovery_strategy,
        }
    }

    /// 检查是否可以重试
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.recovery_strategy,
            RecoveryStrategy::ImmediateRetry
                | RecoveryStrategy::DelayedRetry { .. }
                | RecoveryStrategy::ExponentialBackoff { .. }
        )
    }

    /// 检查是否是致命错误
    pub fn is_fatal(&self) -> bool {
        self.severity == ErrorSeverity::Fatal
    }
}

/// 错误统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorStatistics {
    /// 总错误数
    pub total_errors: u64,
    /// 按类型分组的错误计数
    pub errors_by_type: HashMap<String, u64>,
    /// 按严重程度分组的错误计数
    pub errors_by_severity: HashMap<ErrorSeverity, u64>,
    /// 按组件分组的错误计数
    pub errors_by_component: HashMap<String, u64>,
    /// 最近的错误
    pub recent_errors: Vec<ContextualError>,
    /// 统计时间窗口
    pub time_window: Duration,
    /// 统计开始时间
    pub start_time: SystemTime,
}

impl ErrorStatistics {
    /// 创建新的错误统计
    pub fn new(time_window: Duration) -> Self {
        Self {
            total_errors: 0,
            errors_by_type: HashMap::new(),
            errors_by_severity: HashMap::new(),
            errors_by_component: HashMap::new(),
            recent_errors: Vec::new(),
            time_window,
            start_time: SystemTime::now(),
        }
    }

    /// 记录错误
    pub fn record_error(&mut self, error: &ContextualError) {
        self.total_errors += 1;

        // 按类型统计
        let error_type = format!("{:?}", error.error);
        *self.errors_by_type.entry(error_type).or_insert(0) += 1;

        // 按严重程度统计
        *self.errors_by_severity.entry(error.severity).or_insert(0) += 1;

        // 按组件统计
        *self
            .errors_by_component
            .entry(error.context.component.clone())
            .or_insert(0) += 1;

        // 记录最近的错误（保持最近100个）
        self.recent_errors.push(error.clone());
        if self.recent_errors.len() > 100 {
            self.recent_errors.remove(0);
        }
    }

    /// 获取错误率
    pub fn get_error_rate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().unwrap_or(Duration::from_secs(1));
        self.total_errors as f64 / elapsed.as_secs() as f64
    }

    /// 清理过期的错误记录
    pub fn cleanup_expired(&mut self) {
        let cutoff = SystemTime::now() - self.time_window;
        self.recent_errors.retain(|e| e.timestamp > cutoff);
    }
}

/// 错误分类器 - 根据错误类型确定严重程度和恢复策略
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// 分类错误并确定恢复策略
    pub fn classify_error(error: &VideoProcessorError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            VideoProcessorError::Network(net_err) => Self::classify_network_error(net_err),
            VideoProcessorError::Parsing(parse_err) => Self::classify_parsing_error(parse_err),
            VideoProcessorError::Processing(proc_err) => Self::classify_processing_error(proc_err),
            VideoProcessorError::Configuration(config_err) => {
                Self::classify_config_error(config_err)
            }
            VideoProcessorError::System(sys_err) => Self::classify_system_error(sys_err),
            VideoProcessorError::Resource(res_err) => Self::classify_resource_error(res_err),
        }
    }

    fn classify_network_error(error: &NetworkError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            NetworkError::ConnectionTimeout { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::ExponentialBackoff {
                    base_delay: Duration::from_secs(1),
                    max_retries: 3,
                },
            ),
            NetworkError::DnsResolutionFailed { .. } => (
                ErrorSeverity::Error,
                RecoveryStrategy::Fallback {
                    alternative: "alternative_dns".to_string(),
                },
            ),
            NetworkError::HttpError { status, .. } => {
                if *status >= 500 {
                    (
                        ErrorSeverity::Warning,
                        RecoveryStrategy::DelayedRetry {
                            delay: Duration::from_secs(5),
                        },
                    )
                } else if *status >= 400 {
                    (ErrorSeverity::Error, RecoveryStrategy::Skip)
                } else {
                    (ErrorSeverity::Info, RecoveryStrategy::ImmediateRetry)
                }
            }
            NetworkError::NetworkUnreachable { .. } => (
                ErrorSeverity::Critical,
                RecoveryStrategy::ManualIntervention,
            ),
            NetworkError::SslError { .. } => (
                ErrorSeverity::Error,
                RecoveryStrategy::Fallback {
                    alternative: "http_fallback".to_string(),
                },
            ),
        }
    }

    fn classify_parsing_error(error: &ParsingError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            ParsingError::InvalidM3U8Format { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::Degrade)
            }
            ParsingError::EncodingError { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::Fallback {
                    alternative: "auto_detect_encoding".to_string(),
                },
            ),
            ParsingError::StructureError { .. } => {
                (ErrorSeverity::Warning, RecoveryStrategy::Degrade)
            }
            ParsingError::MissingRequiredField { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::Skip)
            }
        }
    }

    fn classify_processing_error(error: &ProcessingError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            ProcessingError::FFmpegCrash { .. } => (
                ErrorSeverity::Critical,
                RecoveryStrategy::ExponentialBackoff {
                    base_delay: Duration::from_secs(2),
                    max_retries: 2,
                },
            ),
            ProcessingError::DecryptionFailed { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::Skip)
            }
            ProcessingError::EncodingFailed { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::Fallback {
                    alternative: "software_encoding".to_string(),
                },
            ),
            ProcessingError::SegmentProcessingFailed { .. } => {
                (ErrorSeverity::Warning, RecoveryStrategy::ImmediateRetry)
            }
        }
    }

    fn classify_config_error(error: &ConfigError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            ConfigError::ConfigFileNotFound { .. } => (
                ErrorSeverity::Error,
                RecoveryStrategy::Fallback {
                    alternative: "default_config".to_string(),
                },
            ),
            ConfigError::ValidationFailed { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::ManualIntervention)
            }
            ConfigError::ConfigConflict { .. } => {
                (ErrorSeverity::Warning, RecoveryStrategy::Degrade)
            }
            ConfigError::InvalidValue { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::Fallback {
                    alternative: "default_value".to_string(),
                },
            ),
        }
    }

    fn classify_system_error(error: &SystemError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            SystemError::OutOfMemory { .. } => (ErrorSeverity::Critical, RecoveryStrategy::Degrade),
            SystemError::InsufficientDiskSpace { .. } => {
                (ErrorSeverity::Critical, RecoveryStrategy::Stop)
            }
            SystemError::FileSystemError { .. } => (
                ErrorSeverity::Error,
                RecoveryStrategy::DelayedRetry {
                    delay: Duration::from_secs(1),
                },
            ),
            SystemError::PermissionDenied { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::ManualIntervention)
            }
        }
    }

    fn classify_resource_error(error: &ResourceError) -> (ErrorSeverity, RecoveryStrategy) {
        match error {
            ResourceError::ResourceUnavailable { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::DelayedRetry {
                    delay: Duration::from_secs(5),
                },
            ),
            ResourceError::ResourceExhausted { .. } => {
                (ErrorSeverity::Error, RecoveryStrategy::Degrade)
            }
            ResourceError::ResourceLocked { .. } => (
                ErrorSeverity::Warning,
                RecoveryStrategy::DelayedRetry {
                    delay: Duration::from_secs(1),
                },
            ),
        }
    }
}

/// 错误处理器 - 统一的错误处理和恢复机制
pub struct ErrorHandler {
    /// 错误统计
    statistics: ErrorStatistics,
    /// 是否启用自动恢复
    auto_recovery_enabled: bool,
}

impl ErrorHandler {
    /// 创建新的错误处理器
    pub fn new(time_window: Duration, auto_recovery_enabled: bool) -> Self {
        Self {
            statistics: ErrorStatistics::new(time_window),
            auto_recovery_enabled,
        }
    }

    /// 处理错误
    pub fn handle_error(
        &mut self,
        error: VideoProcessorError,
        context: ErrorContext,
    ) -> Result<RecoveryStrategy, ContextualError> {
        // 分类错误
        let (severity, recovery_strategy) = ErrorClassifier::classify_error(&error);

        // 创建上下文错误
        let contextual_error =
            ContextualError::new(error, context, severity, recovery_strategy.clone());

        // 记录错误统计
        self.statistics.record_error(&contextual_error);

        // 检查是否是致命错误
        if contextual_error.is_fatal() {
            return Err(contextual_error);
        }

        // 如果启用自动恢复，返回恢复策略
        if self.auto_recovery_enabled {
            Ok(recovery_strategy)
        } else {
            Err(contextual_error)
        }
    }

    /// 获取错误统计
    pub fn get_statistics(&self) -> &ErrorStatistics {
        &self.statistics
    }

    /// 清理过期的错误记录
    pub fn cleanup(&mut self) {
        self.statistics.cleanup_expired();
    }

    /// 检查系统健康状态
    pub fn check_health(&self) -> SystemHealth {
        let error_rate = self.statistics.get_error_rate();
        let critical_errors = self
            .statistics
            .errors_by_severity
            .get(&ErrorSeverity::Critical)
            .unwrap_or(&0);
        let fatal_errors = self
            .statistics
            .errors_by_severity
            .get(&ErrorSeverity::Fatal)
            .unwrap_or(&0);

        if *fatal_errors > 0 {
            SystemHealth::Critical
        } else if *critical_errors > 5 || error_rate > 10.0 {
            SystemHealth::Degraded
        } else if error_rate > 1.0 {
            SystemHealth::Warning
        } else {
            SystemHealth::Healthy
        }
    }
}

/// 系统健康状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemHealth {
    /// 系统健康
    Healthy,
    /// 系统有警告
    Warning,
    /// 系统性能下降
    Degraded,
    /// 系统严重问题
    Critical,
}

impl fmt::Display for SystemHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemHealth::Healthy => write!(f, "健康"),
            SystemHealth::Warning => write!(f, "警告"),
            SystemHealth::Degraded => write!(f, "性能下降"),
            SystemHealth::Critical => write!(f, "严重问题"),
        }
    }
}
