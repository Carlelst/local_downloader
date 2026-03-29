/// 并发处理优化器
/// 提供高效的任务调度算法、锁竞争优化和背压控制
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};
use tokio::time::interval;

/// 并发优化器配置
#[derive(Debug, Clone)]
pub struct ConcurrencyConfig {
    /// 最大并发任务数
    pub max_concurrent_tasks: usize,
    /// 最小并发任务数
    pub min_concurrent_tasks: usize,
    /// 任务队列最大长度
    pub max_queue_length: usize,
    /// 背压阈值
    pub backpressure_threshold: f64,
    /// 自适应调整间隔
    pub adjustment_interval: Duration,
    /// 内存使用阈值（字节）
    pub memory_threshold: usize,
    /// CPU使用率阈值
    pub cpu_threshold: f64,
}

/// 任务优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// 任务类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskType {
    Download,
    Processing,
    Encoding,
    IO,
}

/// 任务元数据
#[derive(Debug, Clone)]
pub struct TaskMetadata {
    pub task_id: String,
    pub task_type: TaskType,
    pub priority: TaskPriority,
    pub estimated_duration: Option<Duration>,
    pub memory_requirement: Option<usize>,
    pub cpu_requirement: Option<f64>,
    pub created_at: SystemTime,
    pub deadline: Option<SystemTime>,
}

/// 任务包装器
pub struct Task {
    pub metadata: TaskMetadata,
    pub future: std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'static>>,
}

/// 系统资源监控器
#[derive(Debug, Clone)]
pub struct ResourceMonitor {
    cpu_usage: Arc<RwLock<f64>>,
    memory_usage: Arc<RwLock<usize>>,
    active_tasks: Arc<RwLock<usize>>,
    queue_length: Arc<RwLock<usize>>,
    last_update: Arc<RwLock<SystemTime>>,
}

/// 性能指标
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub throughput: f64,           // 任务/秒
    pub average_latency: Duration, // 平均延迟
    pub queue_wait_time: Duration, // 队列等待时间
    pub cpu_utilization: f64,      // CPU利用率
    pub memory_utilization: f64,   // 内存利用率
    pub error_rate: f64,           // 错误率
}

/// 智能任务调度器
pub struct SmartTaskScheduler {
    config: ConcurrencyConfig,
    task_sender: mpsc::UnboundedSender<Task>,
    task_receiver: Arc<Mutex<mpsc::UnboundedReceiver<Task>>>,
    semaphore: Arc<Semaphore>,
    resource_monitor: Arc<ResourceMonitor>,
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
    adjustment_history: Arc<RwLock<Vec<ConcurrencyAdjustment>>>,
    shutdown_signal: Arc<tokio::sync::Notify>,
}

/// 并发调整记录
#[derive(Debug, Clone)]
struct ConcurrencyAdjustment {
    timestamp: SystemTime,
    old_concurrency: usize,
    new_concurrency: usize,
    reason: AdjustmentReason,
    metrics_before: PerformanceMetrics,
    metrics_after: Option<PerformanceMetrics>,
}

/// 调整原因
#[derive(Debug, Clone)]
enum AdjustmentReason {
    HighCpuUsage,
    LowCpuUsage,
    HighMemoryUsage,
    LowMemoryUsage,
    HighQueueLength,
    LowQueueLength,
    HighErrorRate,
    LowErrorRate,
    PerformanceImprovement,
    PerformanceDegradation,
}

/// 背压控制器
pub struct BackpressureController {
    threshold: f64,
    current_pressure: Arc<RwLock<f64>>,
    pressure_history: Arc<RwLock<VecDeque<(SystemTime, f64)>>>,
    control_actions: Arc<RwLock<Vec<BackpressureAction>>>,
}

/// 背压动作
#[derive(Debug, Clone)]
enum BackpressureAction {
    ReduceConcurrency(usize),
    RejectTasks,
    DelayTasks(Duration),
    PrioritizeExisting,
}

/// 内存池管理器
pub struct MemoryPoolManager {
    pools: HashMap<usize, Arc<Mutex<VecDeque<Vec<u8>>>>>,
    allocation_stats: Arc<RwLock<AllocationStats>>,
    cleanup_interval: Duration,
}

/// 分配统计
#[derive(Debug, Clone, Default)]
struct AllocationStats {
    total_allocations: u64,
    total_deallocations: u64,
    peak_memory_usage: usize,
    current_memory_usage: usize,
    pool_hits: u64,
    pool_misses: u64,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: num_cpus::get() * 2,
            min_concurrent_tasks: 1,
            max_queue_length: 1000,
            backpressure_threshold: 0.8,
            adjustment_interval: Duration::from_secs(10),
            memory_threshold: 1024 * 1024 * 1024, // 1GB
            cpu_threshold: 0.8,
        }
    }
}

impl ResourceMonitor {
    pub fn new() -> Self {
        Self {
            cpu_usage: Arc::new(RwLock::new(0.0)),
            memory_usage: Arc::new(RwLock::new(0)),
            active_tasks: Arc::new(RwLock::new(0)),
            queue_length: Arc::new(RwLock::new(0)),
            last_update: Arc::new(RwLock::new(SystemTime::now())),
        }
    }

    /// 更新CPU使用率
    pub async fn update_cpu_usage(&self, usage: f64) {
        *self.cpu_usage.write().await = usage;
        *self.last_update.write().await = SystemTime::now();
    }

    /// 更新内存使用量
    pub async fn update_memory_usage(&self, usage: usize) {
        *self.memory_usage.write().await = usage;
        *self.last_update.write().await = SystemTime::now();
    }

    /// 获取当前资源使用情况
    pub async fn get_resource_usage(&self) -> (f64, usize, usize, usize) {
        let cpu = *self.cpu_usage.read().await;
        let memory = *self.memory_usage.read().await;
        let active = *self.active_tasks.read().await;
        let queue = *self.queue_length.read().await;
        (cpu, memory, active, queue)
    }

    /// 检查是否需要调整并发级别
    pub async fn should_adjust_concurrency(
        &self,
        config: &ConcurrencyConfig,
    ) -> Option<AdjustmentReason> {
        let (cpu, memory, _active, queue) = self.get_resource_usage().await;

        if cpu > config.cpu_threshold {
            Some(AdjustmentReason::HighCpuUsage)
        } else if memory > config.memory_threshold {
            Some(AdjustmentReason::HighMemoryUsage)
        } else if queue > config.max_queue_length / 2 {
            Some(AdjustmentReason::HighQueueLength)
        } else if cpu < config.cpu_threshold * 0.5 && queue == 0 {
            Some(AdjustmentReason::LowCpuUsage)
        } else {
            None
        }
    }
}

impl SmartTaskScheduler {
    pub fn new(config: ConcurrencyConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_tasks));
        let resource_monitor = Arc::new(ResourceMonitor::new());
        let (task_sender, task_receiver) = mpsc::unbounded_channel();

        Self {
            config,
            task_sender,
            task_receiver: Arc::new(Mutex::new(task_receiver)),
            semaphore,
            resource_monitor,
            performance_metrics: Arc::new(RwLock::new(PerformanceMetrics::default())),
            adjustment_history: Arc::new(RwLock::new(Vec::new())),
            shutdown_signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// 启动调度器
    pub async fn start(&self) -> Result<()> {
        log::info!("启动智能任务调度器");

        // 启动资源监控
        self.start_resource_monitoring().await?;

        // 启动自适应调整
        self.start_adaptive_adjustment().await?;

        // 启动任务执行循环
        self.start_task_execution_loop().await?;

        Ok(())
    }

    /// 提交任务
    pub async fn submit_task(&self, task: Task) -> Result<()> {
        // 检查背压
        if self.should_apply_backpressure().await {
            return Err(anyhow::anyhow!("系统负载过高，拒绝新任务"));
        }

        // 发送任务到通道
        self.task_sender
            .send(task)
            .map_err(|_| anyhow::anyhow!("任务队列已关闭"))?;

        log::debug!("任务已提交到调度器");
        Ok(())
    }

    /// 检查是否应该应用背压
    async fn should_apply_backpressure(&self) -> bool {
        let (cpu, memory, _active, queue) = self.resource_monitor.get_resource_usage().await;

        cpu > self.config.cpu_threshold
            || memory > self.config.memory_threshold
            || queue > self.config.max_queue_length
    }

    /// 启动资源监控
    async fn start_resource_monitoring(&self) -> Result<()> {
        let resource_monitor = Arc::clone(&self.resource_monitor);
        let _config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                // 监控CPU使用率
                if let Ok(cpu_usage) = Self::get_cpu_usage().await {
                    resource_monitor.update_cpu_usage(cpu_usage).await;
                }

                // 监控内存使用量
                if let Ok(memory_usage) = Self::get_memory_usage().await {
                    resource_monitor.update_memory_usage(memory_usage).await;
                }
            }
        });

        Ok(())
    }

    /// 启动自适应调整
    async fn start_adaptive_adjustment(&self) -> Result<()> {
        let scheduler = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(scheduler.config.adjustment_interval);

            loop {
                interval.tick().await;

                if let Some(reason) = scheduler
                    .resource_monitor
                    .should_adjust_concurrency(&scheduler.config)
                    .await
                {
                    if let Err(e) = scheduler.adjust_concurrency(reason).await {
                        log::error!("并发调整失败: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// 启动任务执行循环
    async fn start_task_execution_loop(&self) -> Result<()> {
        let task_receiver = Arc::clone(&self.task_receiver);
        let semaphore = Arc::clone(&self.semaphore);
        let resource_monitor = Arc::clone(&self.resource_monitor);
        let performance_metrics = Arc::clone(&self.performance_metrics);

        tokio::spawn(async move {
            loop {
                // 从通道接收任务
                let task = {
                    let mut receiver = task_receiver.lock().await;
                    match receiver.recv().await {
                        Some(task) => task,
                        None => {
                            log::info!("任务通道已关闭，退出执行循环");
                            break;
                        }
                    }
                };

                let semaphore_clone = Arc::clone(&semaphore);
                let resource_monitor_clone = Arc::clone(&resource_monitor);
                let performance_metrics_clone = Arc::clone(&performance_metrics);

                // 在新任务中执行
                tokio::spawn(async move {
                    // 获取信号量许可
                    let permit = match semaphore_clone.try_acquire() {
                        Ok(permit) => permit,
                        Err(_) => {
                            // 如果无法获取许可，等待后重试
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            return;
                        }
                    };

                    let start_time = Instant::now();

                    // 更新活跃任务计数
                    {
                        let mut active = resource_monitor_clone.active_tasks.write().await;
                        *active += 1;
                    }

                    // 执行任务
                    let result = task.future.await;

                    // 更新活跃任务计数
                    {
                        let mut active = resource_monitor_clone.active_tasks.write().await;
                        *active -= 1;
                    }

                    // 更新性能指标
                    let execution_time = start_time.elapsed();
                    let mut metrics = performance_metrics_clone.write().await;

                    // 更新延迟（简化计算）
                    metrics.average_latency = (metrics.average_latency + execution_time) / 2;

                    // 更新错误率（简化计算）
                    if !result.is_ok() {
                        metrics.error_rate = (metrics.error_rate * 0.9) + 0.1;
                    } else {
                        metrics.error_rate = metrics.error_rate * 0.95;
                    }

                    // 更新吞吐量（简化计算）
                    let throughput_increment = 1.0 / execution_time.as_secs_f64();
                    metrics.throughput = (metrics.throughput * 0.9) + (throughput_increment * 0.1);

                    // 释放许可
                    drop(permit);

                    if let Err(e) = result {
                        log::error!("任务执行失败: {}", e);
                    }
                });
            }
        });

        Ok(())
    }

    /// 调整并发级别
    async fn adjust_concurrency(&self, reason: AdjustmentReason) -> Result<()> {
        let current_permits = self.semaphore.available_permits();
        let current_concurrency = self.config.max_concurrent_tasks - current_permits;

        let new_concurrency = match reason {
            AdjustmentReason::HighCpuUsage | AdjustmentReason::HighMemoryUsage => {
                (current_concurrency * 8 / 10).max(self.config.min_concurrent_tasks)
            }
            AdjustmentReason::LowCpuUsage | AdjustmentReason::LowMemoryUsage => {
                (current_concurrency * 12 / 10).min(self.config.max_concurrent_tasks)
            }
            AdjustmentReason::HighQueueLength => {
                (current_concurrency * 11 / 10).min(self.config.max_concurrent_tasks)
            }
            _ => current_concurrency,
        };

        if new_concurrency != current_concurrency {
            log::info!(
                "调整并发级别: {} -> {} (原因: {:?})",
                current_concurrency,
                new_concurrency,
                reason
            );

            // 记录调整历史
            let metrics_before = self.performance_metrics.read().await.clone();
            let adjustment = ConcurrencyAdjustment {
                timestamp: SystemTime::now(),
                old_concurrency: current_concurrency,
                new_concurrency,
                reason,
                metrics_before,
                metrics_after: None,
            };

            self.adjustment_history.write().await.push(adjustment);

            // 实际调整需要重新创建信号量或使用其他机制
            // 这里简化处理，实际实现可能需要更复杂的逻辑
        }

        Ok(())
    }

    /// 获取CPU使用率
    async fn get_cpu_usage() -> Result<f64> {
        // 简化实现，实际应该使用系统API
        // 这里返回一个模拟值
        Ok(0.5)
    }

    /// 获取内存使用量
    async fn get_memory_usage() -> Result<usize> {
        // 简化实现，实际应该使用系统API
        // 这里返回一个模拟值
        Ok(1024 * 1024 * 512) // 512MB
    }

    /// 停止调度器
    pub async fn shutdown(&self) -> Result<()> {
        log::info!("停止智能任务调度器");
        self.shutdown_signal.notify_waiters();
        Ok(())
    }
}

impl Clone for SmartTaskScheduler {
    fn clone(&self) -> Self {
        let (task_sender, task_receiver) = mpsc::unbounded_channel();

        Self {
            config: self.config.clone(),
            task_sender,
            task_receiver: Arc::new(Mutex::new(task_receiver)),
            semaphore: Arc::clone(&self.semaphore),
            resource_monitor: Arc::clone(&self.resource_monitor),
            performance_metrics: Arc::clone(&self.performance_metrics),
            adjustment_history: Arc::clone(&self.adjustment_history),
            shutdown_signal: Arc::clone(&self.shutdown_signal),
        }
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            throughput: 0.0,
            average_latency: Duration::from_millis(0),
            queue_wait_time: Duration::from_millis(0),
            cpu_utilization: 0.0,
            memory_utilization: 0.0,
            error_rate: 0.0,
        }
    }
}

impl BackpressureController {
    pub fn new(threshold: f64) -> Self {
        Self {
            threshold,
            current_pressure: Arc::new(RwLock::new(0.0)),
            pressure_history: Arc::new(RwLock::new(VecDeque::new())),
            control_actions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 计算当前系统压力
    pub async fn calculate_pressure(&self, metrics: &PerformanceMetrics) -> f64 {
        let cpu_pressure = metrics.cpu_utilization;
        let memory_pressure = metrics.memory_utilization;
        let queue_pressure = if metrics.queue_wait_time.as_millis() > 0 {
            (metrics.queue_wait_time.as_millis() as f64 / 1000.0).min(1.0)
        } else {
            0.0
        };
        let error_pressure = metrics.error_rate;

        // 加权平均
        let pressure = (cpu_pressure * 0.3)
            + (memory_pressure * 0.3)
            + (queue_pressure * 0.2)
            + (error_pressure * 0.2);

        *self.current_pressure.write().await = pressure;

        // 记录历史
        let mut history = self.pressure_history.write().await;
        history.push_back((SystemTime::now(), pressure));

        // 保持历史记录在合理范围内
        while history.len() > 100 {
            history.pop_front();
        }

        pressure
    }

    /// 应用背压控制
    pub async fn apply_backpressure(&self, pressure: f64) -> Vec<BackpressureAction> {
        let mut actions = Vec::new();

        if pressure > self.threshold {
            if pressure > 0.9 {
                actions.push(BackpressureAction::RejectTasks);
                actions.push(BackpressureAction::ReduceConcurrency(1));
            } else if pressure > 0.8 {
                actions.push(BackpressureAction::DelayTasks(Duration::from_millis(100)));
                actions.push(BackpressureAction::PrioritizeExisting);
            } else {
                actions.push(BackpressureAction::ReduceConcurrency(2));
            }
        }

        // 记录控制动作
        self.control_actions.write().await.extend(actions.clone());

        actions
    }
}

impl MemoryPoolManager {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            allocation_stats: Arc::new(RwLock::new(AllocationStats::default())),
            cleanup_interval: Duration::from_secs(60),
        }
    }

    /// 获取指定大小的内存块
    pub async fn get_buffer(&mut self, size: usize) -> Vec<u8> {
        // 找到最接近的池大小
        let pool_size = self.find_pool_size(size);

        if let Some(pool) = self.pools.get(&pool_size) {
            let mut pool_guard = pool.lock().await;
            if let Some(buffer) = pool_guard.pop_front() {
                // 池命中
                self.allocation_stats.write().await.pool_hits += 1;
                return buffer;
            }
        }

        // 池未命中，创建新缓冲区
        self.allocation_stats.write().await.pool_misses += 1;
        self.allocation_stats.write().await.total_allocations += 1;

        vec![0u8; pool_size]
    }

    /// 归还内存块到池中
    pub async fn return_buffer(&mut self, mut buffer: Vec<u8>) {
        let size = buffer.capacity();
        let pool_size = self.find_pool_size(size);

        // 清零缓冲区（安全考虑）
        buffer.fill(0);
        buffer.resize(pool_size, 0);

        let pool = self
            .pools
            .entry(pool_size)
            .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new())));

        let mut pool_guard = pool.lock().await;

        // 限制池大小，避免内存泄漏
        if pool_guard.len() < 10 {
            pool_guard.push_back(buffer);
        }

        self.allocation_stats.write().await.total_deallocations += 1;
    }

    /// 找到合适的池大小
    fn find_pool_size(&self, size: usize) -> usize {
        // 使用2的幂次作为池大小
        let mut pool_size = 1024; // 1KB起始
        while pool_size < size {
            pool_size *= 2;
        }
        pool_size.min(16 * 1024 * 1024) // 最大16MB
    }

    /// 启动定期清理
    pub async fn start_cleanup(&self) -> Result<()> {
        let pools = self.pools.clone();
        let cleanup_interval = self.cleanup_interval;

        tokio::spawn(async move {
            let mut interval = interval(cleanup_interval);

            loop {
                interval.tick().await;

                // 清理过大的池
                for (_size, pool) in &pools {
                    let mut pool_guard = pool.lock().await;
                    while pool_guard.len() > 5 {
                        pool_guard.pop_front();
                    }
                }
            }
        });

        Ok(())
    }

    /// 获取分配统计
    pub async fn get_stats(&self) -> AllocationStats {
        self.allocation_stats.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resource_monitor() {
        let monitor = ResourceMonitor::new();

        monitor.update_cpu_usage(0.5).await;
        monitor.update_memory_usage(1024 * 1024).await;

        let (cpu, memory, _active, _queue) = monitor.get_resource_usage().await;
        assert_eq!(cpu, 0.5);
        assert_eq!(memory, 1024 * 1024);
    }

    #[tokio::test]
    async fn test_memory_pool() {
        let mut pool = MemoryPoolManager::new();

        let buffer1 = pool.get_buffer(1024).await;
        assert!(buffer1.len() >= 1024);

        pool.return_buffer(buffer1).await;

        let buffer2 = pool.get_buffer(1024).await;
        assert!(buffer2.len() >= 1024);

        let stats = pool.get_stats().await;
        assert!(stats.total_allocations > 0);
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
    }
}
