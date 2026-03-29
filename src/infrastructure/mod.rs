pub mod cache_manager;
pub mod concurrent_optimizer;
pub mod download;
pub mod external;
pub mod ffmpeg_handler;
pub mod filesystem;
/// 基础设施层 - 外部服务集成和数据持久化
///
/// 该层负责：
/// - 实现领域服务接口的具体实现
/// - 外部系统集成（HTTP客户端、文件系统等）
/// - 数据持久化和缓存
/// - 第三方库的封装
pub mod http;
pub mod logger;
pub mod logging_integration;
pub mod m3u8_parser;
pub mod metrics_collector;
pub mod persistence;
pub mod progress_monitor;
pub mod resource_monitor;
pub mod segment_processor;
pub mod state_persistence;
pub mod subtitle_processor;

pub use cache_manager::{
    AdaptiveCacheManager, CacheConfig, CacheStats, CacheStrategy, ConnectionPoolConfig,
    HttpConnectionPool, LRUCache,
};
pub use concurrent_optimizer::{
    BackpressureController, ConcurrencyConfig, MemoryPoolManager, PerformanceMetrics,
    ResourceMonitor as ConcurrentResourceMonitor, SmartTaskScheduler, Task, TaskMetadata,
    TaskPriority, TaskType,
};
pub use download::{
    AdaptiveDownloadManager, BandwidthMonitor, ConcurrencyController, DownloadConfig, DownloadTask,
    ErrorClassifier, ErrorType, RetryStrategy,
};
pub use external::*;
pub use ffmpeg_handler::DefaultFFmpegHandler;
pub use filesystem::{
    DefaultFileSystem, FileMetadata, FilePermissions, FileSystem, FileSystemError, TempFileManager,
};
pub use http::{DefaultHttpClient, HttpClient, HttpError, HttpResponse};
pub use logger::{
    create_context, LogEntry, LogFormat, LogLevel, LogOutput, LogRotationConfig, LoggerConfig,
    SimpleLogger,
};
pub use logging_integration::{
    create_app_logger_config, init_console_logger, init_file_logger, init_hybrid_logger,
    parse_log_level_string,
};
pub use m3u8_parser::DefaultM3U8Parser;
pub use metrics_collector::{
    AggregatedMetrics, DefaultMetricsCollector, GlobalMetrics as MetricsGlobalStats, MetricStats,
    MetricType, MetricsCollector, MetricsCollectorConfig, PerformanceRecommendation,
    PerformanceReport, SystemPerformanceSummary, TaskPerformanceSummary, TrendAnalysis,
};
pub use persistence::{
    ApplicationState, BackupManager, CacheManager, DownloadProgress, DownloadStatus,
    InMemoryPersistence, JsonFilePersistence, PersistenceStore, PersistenceStoreExt, StateManager,
};
pub use progress_monitor::{
    DefaultProgressMonitor, EstimationAlgorithm, EstimationState, GlobalStats,
    ProgressMonitorConfig, ProgressSnapshot, StatsCollector, TaskProgress, TrendData,
};
pub use resource_monitor::{
    CleanupPolicy, CpuUsage, DiskUsage, MemoryUsage, MonitorType, ResourceAlert, ResourceMonitor,
    ResourceMonitorConfig, ResourceSnapshot, ResourceThresholds, SystemResourceMonitor,
};
pub use segment_processor::DefaultSegmentProcessor;
pub use state_persistence::{
    ApplicationStateSnapshot, BackupConfig, ConfigurationState, DownloadState, ProcessingState,
    RuntimeStats, StatePersistence, StatePersistenceManager, StateRecoveryResult, StateStatistics,
    SystemState, TaskStatus,
};
pub use subtitle_processor::DefaultSubtitleProcessor;
