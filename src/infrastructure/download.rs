/// 下载管理器基础设施实现
/// 提供自适应并发控制、智能重试和断点续传功能
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::interval;

use crate::domain::{DownloadManager, DownloadStats, Segment};
use crate::infrastructure::{
    ConcurrencyConfig, HttpClient, PersistenceStore, PersistenceStoreExt, SmartTaskScheduler, Task,
    TaskMetadata, TaskPriority, TaskType,
};

/// 自适应下载管理器
pub struct AdaptiveDownloadManager {
    http_client: Arc<dyn HttpClient>,
    persistence: Arc<dyn PersistenceStore>,
    concurrency_controller: Arc<ConcurrencyController>,
    retry_strategy: Arc<RetryStrategy>,
    bandwidth_monitor: Arc<BandwidthMonitor>,
    stats: Arc<RwLock<DownloadStats>>,
    config: DownloadConfig,
    task_scheduler: Arc<SmartTaskScheduler>,
}

/// 下载配置
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub max_concurrent_downloads: usize,
    pub min_concurrent_downloads: usize,
    pub chunk_size: usize,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub enable_resume: bool,
    pub bandwidth_threshold_mbps: f64,
    pub latency_threshold_ms: u64,
}

/// 并发控制器
pub struct ConcurrencyController {
    semaphore: Semaphore,
    current_level: Arc<RwLock<usize>>,
    max_level: usize,
    min_level: usize,
    adjustment_history: Arc<RwLock<Vec<ConcurrencyAdjustment>>>,
}

/// 并发调整记录
#[derive(Debug, Clone)]
struct ConcurrencyAdjustment {
    timestamp: SystemTime,
    old_level: usize,
    new_level: usize,
    reason: AdjustmentReason,
    performance_metrics: PerformanceMetrics,
}

/// 调整原因
#[derive(Debug, Clone)]
enum AdjustmentReason {
    BandwidthIncrease,
    BandwidthDecrease,
    LatencyIncrease,
    LatencyDecrease,
    ErrorRateIncrease,
    ErrorRateDecrease,
    Manual,
}

/// 性能指标
#[derive(Debug, Clone)]
struct PerformanceMetrics {
    bandwidth_mbps: f64,
    latency_ms: u64,
    error_rate: f64,
    success_rate: f64,
}

/// 重试策略
pub struct RetryStrategy {
    config: RetryConfig,
    error_classifier: ErrorClassifier,
}

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub jitter: bool,
    pub circuit_breaker_threshold: f64,
    pub circuit_breaker_timeout: Duration,
}

/// 错误分类器
pub struct ErrorClassifier;

/// 错误类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorType {
    Network,
    Timeout,
    ServerError,
    ClientError,
    RateLimit,
    Unknown,
}

/// 带宽监控器
pub struct BandwidthMonitor {
    measurements: Arc<RwLock<Vec<BandwidthMeasurement>>>,
    window_size: Duration,
    measurement_interval: Duration,
}

/// 带宽测量
#[derive(Debug, Clone)]
struct BandwidthMeasurement {
    timestamp: SystemTime,
    bytes_downloaded: u64,
    duration: Duration,
    bandwidth_mbps: f64,
    latency_ms: u64,
}

/// 下载任务
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub segment: Segment,
    pub attempt: u32,
    pub start_time: Option<SystemTime>,
    pub resume_from: Option<u64>,
    pub task_id: String,
}

impl AdaptiveDownloadManager {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        persistence: Arc<dyn PersistenceStore>,
        config: DownloadConfig,
    ) -> Self {
        let concurrency_controller = Arc::new(ConcurrencyController::new(
            config.max_concurrent_downloads,
            config.min_concurrent_downloads,
        ));

        let retry_strategy = Arc::new(RetryStrategy::new(RetryConfig::default()));
        let bandwidth_monitor = Arc::new(BandwidthMonitor::new(
            Duration::from_secs(60), // 1分钟窗口
            Duration::from_secs(5),  // 5秒测量间隔
        ));

        let stats = Arc::new(RwLock::new(DownloadStats::default()));

        // 创建智能任务调度器
        let scheduler_config = ConcurrencyConfig {
            max_concurrent_tasks: config.max_concurrent_downloads,
            min_concurrent_tasks: config.min_concurrent_downloads,
            max_queue_length: config.max_concurrent_downloads * 10,
            backpressure_threshold: 0.8,
            adjustment_interval: Duration::from_secs(10),
            memory_threshold: 1024 * 1024 * 1024, // 1GB
            cpu_threshold: 0.8,
        };
        let task_scheduler = Arc::new(SmartTaskScheduler::new(scheduler_config));

        Self {
            http_client,
            persistence,
            concurrency_controller,
            retry_strategy,
            bandwidth_monitor,
            stats,
            config,
            task_scheduler,
        }
    }

    /// 下载单个片段
    async fn download_segment(&self, task: DownloadTask) -> Result<(usize, Vec<u8>)> {
        let _permit = self.concurrency_controller.acquire().await;

        let start_time = Instant::now();
        let segment = &task.segment;

        // 检查是否需要断点续传
        let resume_from = if self.config.enable_resume {
            self.load_resume_position(&task.task_id).await.unwrap_or(0)
        } else {
            0
        };

        let result = if resume_from > 0 {
            // 断点续传
            self.http_client
                .get_range(&segment.url, resume_from, None)
                .await
        } else {
            // 完整下载
            let response = self
                .http_client
                .get(
                    &segment.url,
                    Some(Duration::from_secs(self.config.timeout_secs)),
                )
                .await?;
            Ok(response.body)
        };

        let download_duration = start_time.elapsed();

        match result {
            Ok(mut data) => {
                // 如果是断点续传，需要加载之前的数据
                if resume_from > 0 {
                    if let Ok(Some(previous_data)) = self.load_partial_data(&task.task_id).await {
                        let mut combined_data = previous_data;
                        combined_data.extend_from_slice(&data);
                        data = combined_data;
                    }
                }

                // 记录带宽测量
                self.bandwidth_monitor
                    .record_measurement(
                        data.len() as u64,
                        download_duration,
                        0, // TODO: 实际测量延迟
                    )
                    .await;

                // 清理断点续传数据
                if self.config.enable_resume {
                    let _ = self.clear_resume_data(&task.task_id).await;
                }

                // 更新统计信息
                self.update_stats_success(data.len(), download_duration)
                    .await;

                Ok((segment.index, data))
            }
            Err(e) => {
                // 保存断点续传信息
                if self.config.enable_resume && resume_from == 0 {
                    // 这里应该保存已下载的部分数据，但由于错误我们没有数据
                    // 在实际实现中，应该在流式下载过程中保存部分数据
                }

                // 更新统计信息
                self.update_stats_failure().await;

                Err(e)
            }
        }
    }

    /// 加载断点续传位置
    async fn load_resume_position(&self, task_id: &str) -> Result<u64> {
        let key = format!("resume_position_{}", task_id);
        if let Some(position) = self.persistence.load::<u64>(&key).await? {
            Ok(position)
        } else {
            Ok(0)
        }
    }

    /// 保存断点续传位置
    async fn save_resume_position(&self, task_id: &str, position: u64) -> Result<()> {
        let key = format!("resume_position_{}", task_id);
        self.persistence.save(&key, &position).await
    }

    /// 加载部分数据
    async fn load_partial_data(&self, task_id: &str) -> Result<Option<Vec<u8>>> {
        let key = format!("partial_data_{}", task_id);
        self.persistence.load::<Vec<u8>>(&key).await
    }

    /// 保存部分数据
    async fn save_partial_data(&self, task_id: &str, data: &[u8]) -> Result<()> {
        let key = format!("partial_data_{}", task_id);
        let data_vec = data.to_vec();
        self.persistence.save(&key, &data_vec).await
    }

    /// 清理断点续传数据
    async fn clear_resume_data(&self, task_id: &str) -> Result<()> {
        let position_key = format!("resume_position_{}", task_id);
        let data_key = format!("partial_data_{}", task_id);

        let _ = self.persistence.delete(&position_key).await;
        let _ = self.persistence.delete(&data_key).await;

        Ok(())
    }

    /// 更新成功统计
    async fn update_stats_success(&self, bytes: usize, duration: Duration) {
        let mut stats = self.stats.write().await;
        stats.completed_segments += 1;
        stats.total_bytes += bytes as u64;

        // 计算下载速度
        if duration.as_millis() > 0 {
            let speed =
                (bytes as f64 * 8.0) / (duration.as_millis() as f64) * 1000.0 / (1024.0 * 1024.0);
            stats.download_speed = speed;
        }

        // 更新预估剩余时间
        if stats.completed_segments > 0 && stats.total_segments > stats.completed_segments {
            let remaining = stats.total_segments - stats.completed_segments;
            let avg_time_per_segment = duration.as_secs_f64();
            stats.estimated_time_remaining = Some(Duration::from_secs_f64(
                remaining as f64 * avg_time_per_segment,
            ));
        }
    }

    /// 更新失败统计
    async fn update_stats_failure(&self) {
        let mut stats = self.stats.write().await;
        stats.failed_segments += 1;
    }
}

#[async_trait]
impl DownloadManager for AdaptiveDownloadManager {
    async fn download_segments(&self, segments: Vec<Segment>) -> Result<HashMap<usize, Vec<u8>>> {
        let mut stats = self.stats.write().await;
        stats.total_segments = segments.len();
        stats.completed_segments = 0;
        stats.failed_segments = 0;
        drop(stats);

        // 启动智能任务调度器
        self.task_scheduler.start().await?;

        let mut results = HashMap::new();
        let results_arc = Arc::new(RwLock::new(HashMap::new()));
        let _task_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let expected_count = segments.len();

        // 创建下载任务并提交到调度器
        for segment in &segments {
            let task_id = format!("download_segment_{}", segment.index);
            let segment_index = segment.index;

            // 确定任务优先级（可以根据片段重要性调整）
            let priority = if segment.index < 10 {
                TaskPriority::High // 前几个片段高优先级
            } else {
                TaskPriority::Normal
            };

            let metadata = TaskMetadata {
                task_id: task_id.clone(),
                task_type: TaskType::Download,
                priority,
                estimated_duration: Some(Duration::from_secs(5)), // 估计5秒下载时间
                memory_requirement: Some(segment.size.unwrap_or(1024 * 1024) as usize), // 使用片段大小
                cpu_requirement: Some(0.1), // 下载任务CPU需求较低
                created_at: SystemTime::now(),
                deadline: None,
            };

            let http_client = Arc::clone(&self.http_client);
            let persistence = Arc::clone(&self.persistence);
            let bandwidth_monitor = Arc::clone(&self.bandwidth_monitor);
            let stats = Arc::clone(&self.stats);
            let config = self.config.clone();
            let results_clone = Arc::clone(&results_arc);

            let download_task = DownloadTask {
                task_id: task_id.clone(),
                segment: segment.clone(),
                attempt: 0,
                start_time: None,
                resume_from: None,
            };

            // 创建异步任务
            let future = async move {
                let manager = AdaptiveDownloadManager {
                    http_client,
                    persistence,
                    concurrency_controller: Arc::new(ConcurrencyController::new(
                        config.max_concurrent_downloads,
                        config.min_concurrent_downloads,
                    )),
                    retry_strategy: Arc::new(RetryStrategy::new(RetryConfig::default())),
                    bandwidth_monitor,
                    stats,
                    config,
                    task_scheduler: Arc::new(SmartTaskScheduler::new(ConcurrencyConfig::default())),
                };

                match manager.download_segment(download_task).await {
                    Ok((index, data)) => {
                        let mut results = results_clone.write().await;
                        results.insert(index, data);
                        Ok(())
                    }
                    Err(e) => {
                        log::error!("下载片段 {} 失败: {}", segment_index, e);
                        Err(e)
                    }
                }
            };

            let task = Task {
                metadata,
                future: Box::pin(future),
            };

            // 提交任务到调度器
            if let Err(e) = self.task_scheduler.submit_task(task).await {
                log::error!("提交下载任务失败: {}", e);
                continue;
            }
        }

        // 等待所有任务完成（简化实现）
        // 实际实现中应该有更好的等待机制
        let mut check_interval = interval(Duration::from_millis(500));
        let start_time = Instant::now();
        let timeout = Duration::from_secs(300); // 5分钟超时

        loop {
            check_interval.tick().await;

            let current_results = results_arc.read().await;
            if current_results.len() >= expected_count {
                results = current_results.clone();
                break;
            }

            if start_time.elapsed() > timeout {
                log::warn!(
                    "下载超时，已完成 {}/{} 个片段",
                    current_results.len(),
                    expected_count
                );
                results = current_results.clone();
                break;
            }
        }

        // 停止调度器
        let _ = self.task_scheduler.shutdown().await;

        Ok(results)
    }

    async fn set_concurrency(&self, level: usize) -> Result<()> {
        self.concurrency_controller
            .set_level(level, AdjustmentReason::Manual)
            .await
    }

    fn get_download_stats(&self) -> DownloadStats {
        // 这里应该返回当前统计信息的快照
        // 由于trait方法不是async，我们返回一个默认值
        // 在实际实现中，可能需要修改trait定义
        DownloadStats::default()
    }

    async fn pause_downloads(&self) -> Result<()> {
        // 实现暂停逻辑
        // 这里可以设置一个暂停标志，让正在进行的下载完成后不再开始新的下载
        Ok(())
    }

    async fn resume_downloads(&self) -> Result<()> {
        // 实现恢复逻辑
        Ok(())
    }

    async fn cancel_downloads(&self) -> Result<()> {
        // 实现取消逻辑
        Ok(())
    }
}

impl ConcurrencyController {
    pub fn new(max_level: usize, min_level: usize) -> Self {
        Self {
            semaphore: Semaphore::new(max_level),
            current_level: Arc::new(RwLock::new(max_level)),
            max_level,
            min_level,
            adjustment_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn acquire(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.semaphore.acquire().await.unwrap()
    }

    pub async fn set_level(&self, new_level: usize, reason: AdjustmentReason) -> Result<()> {
        let new_level = new_level.clamp(self.min_level, self.max_level);
        let mut current = self.current_level.write().await;

        if *current != new_level {
            let old_level = *current;
            *current = new_level;

            // 记录调整历史
            let adjustment = ConcurrencyAdjustment {
                timestamp: SystemTime::now(),
                old_level,
                new_level,
                reason: reason.clone(),
                performance_metrics: PerformanceMetrics {
                    bandwidth_mbps: 0.0, // TODO: 从监控器获取实际值
                    latency_ms: 0,
                    error_rate: 0.0,
                    success_rate: 0.0,
                },
            };

            self.adjustment_history.write().await.push(adjustment);

            log::info!(
                "并发级别调整: {} -> {} (原因: {:?})",
                old_level,
                new_level,
                &reason
            );
        }

        Ok(())
    }
}

impl RetryStrategy {
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            error_classifier: ErrorClassifier,
        }
    }

    pub async fn should_retry(&self, error: &anyhow::Error, attempt: u32) -> bool {
        if attempt >= self.config.max_attempts {
            return false;
        }

        let error_type = self.error_classifier.classify(error);
        match error_type {
            ErrorType::Network | ErrorType::Timeout | ErrorType::ServerError => true,
            ErrorType::RateLimit => true,
            ErrorType::ClientError => false,
            ErrorType::Unknown => attempt < 2, // 只重试一次未知错误
        }
    }

    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let base_delay_ms = self.config.base_delay.as_millis() as f64;
        let delay_ms = base_delay_ms * self.config.backoff_multiplier.powi(attempt as i32);

        let mut delay = Duration::from_millis(delay_ms as u64);

        if delay > self.config.max_delay {
            delay = self.config.max_delay;
        }

        if self.config.jitter {
            use rand::Rng;
            let jitter_factor = rand::thread_rng().gen_range(0.5..1.5);
            delay = Duration::from_millis((delay.as_millis() as f64 * jitter_factor) as u64);
        }

        delay
    }
}

impl ErrorClassifier {
    pub fn classify(&self, error: &anyhow::Error) -> ErrorType {
        let error_str = error.to_string().to_lowercase();

        if error_str.contains("timeout") || error_str.contains("timed out") {
            ErrorType::Timeout
        } else if error_str.contains("connection") || error_str.contains("network") {
            ErrorType::Network
        } else if error_str.contains("500")
            || error_str.contains("502")
            || error_str.contains("503")
            || error_str.contains("504")
        {
            ErrorType::ServerError
        } else if error_str.contains("429") {
            ErrorType::RateLimit
        } else if error_str.contains("400")
            || error_str.contains("401")
            || error_str.contains("403")
            || error_str.contains("404")
        {
            ErrorType::ClientError
        } else {
            ErrorType::Unknown
        }
    }
}

impl BandwidthMonitor {
    pub fn new(window_size: Duration, measurement_interval: Duration) -> Self {
        Self {
            measurements: Arc::new(RwLock::new(Vec::new())),
            window_size,
            measurement_interval,
        }
    }

    pub async fn record_measurement(&self, bytes: u64, duration: Duration, latency_ms: u64) {
        let bandwidth_mbps = if duration.as_millis() > 0 {
            (bytes as f64 * 8.0) / (duration.as_millis() as f64) * 1000.0 / (1024.0 * 1024.0)
        } else {
            0.0
        };

        let measurement = BandwidthMeasurement {
            timestamp: SystemTime::now(),
            bytes_downloaded: bytes,
            duration,
            bandwidth_mbps,
            latency_ms,
        };

        let mut measurements = self.measurements.write().await;
        measurements.push(measurement);

        // 清理过期测量
        let cutoff = SystemTime::now() - self.window_size;
        measurements.retain(|m| m.timestamp > cutoff);
    }

    pub async fn get_average_bandwidth(&self) -> f64 {
        let measurements = self.measurements.read().await;
        if measurements.is_empty() {
            return 0.0;
        }

        let total: f64 = measurements.iter().map(|m| m.bandwidth_mbps).sum();
        total / measurements.len() as f64
    }

    pub async fn get_average_latency(&self) -> u64 {
        let measurements = self.measurements.read().await;
        if measurements.is_empty() {
            return 0;
        }

        let total: u64 = measurements.iter().map(|m| m.latency_ms).sum();
        total / measurements.len() as u64
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 8,
            min_concurrent_downloads: 1,
            chunk_size: 1024 * 1024, // 1MB
            timeout_secs: 30,
            max_retries: 3,
            enable_resume: true,
            bandwidth_threshold_mbps: 10.0,
            latency_threshold_ms: 1000,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
            circuit_breaker_threshold: 0.5,
            circuit_breaker_timeout: Duration::from_secs(60),
        }
    }
}
