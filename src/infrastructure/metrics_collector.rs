/// 性能指标收集器实现
///
/// 该模块实现性能指标的收集、聚合和分析功能，
/// 支持实时监控系统性能和生成详细的性能报告。
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Instant};

use crate::domain::entities::PerformanceMetrics;

/// 默认指标收集器实现
pub struct DefaultMetricsCollector {
    /// 指标存储
    metrics_store: Arc<RwLock<MetricsStore>>,
    /// 收集配置
    config: MetricsCollectorConfig,
    /// 聚合器
    aggregator: Arc<Mutex<MetricsAggregator>>,
    /// 是否正在运行
    is_running: Arc<Mutex<bool>>,
}

/// 指标存储
#[derive(Debug)]
pub struct MetricsStore {
    /// 原始指标数据
    raw_metrics: HashMap<String, VecDeque<PerformanceMetrics>>,
    /// 聚合指标数据
    aggregated_metrics: HashMap<String, AggregatedMetrics>,
    /// 全局指标
    global_metrics: GlobalMetrics,
}

/// 聚合指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedMetrics {
    /// 任务ID
    pub task_id: String,
    /// 时间窗口
    pub time_window: Duration,
    /// CPU使用统计
    pub cpu_stats: MetricStats,
    /// 内存使用统计
    pub memory_stats: MetricStats,
    /// 网络吞吐量统计
    pub network_stats: MetricStats,
    /// 磁盘IO统计
    pub disk_io_stats: MetricStats,
    /// 并发下载统计
    pub concurrency_stats: MetricStats,
    /// 错误率统计
    pub error_rate_stats: MetricStats,
    /// 聚合时间
    pub aggregated_at: SystemTime,
}

/// 指标统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricStats {
    /// 最小值
    pub min: f64,
    /// 最大值
    pub max: f64,
    /// 平均值
    pub mean: f64,
    /// 中位数
    pub median: f64,
    /// 标准差
    pub std_dev: f64,
    /// 第95百分位数
    pub p95: f64,
    /// 第99百分位数
    pub p99: f64,
    /// 样本数量
    pub count: usize,
}

/// 全局指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMetrics {
    /// 系统启动时间
    pub system_start_time: SystemTime,
    /// 总处理任务数
    pub total_tasks_processed: usize,
    /// 当前活跃任务数
    pub active_tasks_count: usize,
    /// 总错误数
    pub total_errors: usize,
    /// 系统平均CPU使用率
    pub system_avg_cpu: f64,
    /// 系统平均内存使用率
    pub system_avg_memory: f64,
    /// 总网络传输量
    pub total_network_bytes: u64,
    /// 总磁盘IO量
    pub total_disk_io_bytes: u64,
}

/// 指标收集配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsCollectorConfig {
    /// 收集间隔
    pub collection_interval: Duration,
    /// 数据保留时间
    pub retention_period: Duration,
    /// 聚合窗口大小
    pub aggregation_windows: Vec<Duration>,
    /// 最大存储的原始指标数量
    pub max_raw_metrics: usize,
    /// 启用的指标类型
    pub enabled_metrics: Vec<MetricType>,
    /// 是否启用实时聚合
    pub enable_real_time_aggregation: bool,
}

/// 指标类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// CPU使用率
    CpuUsage,
    /// 内存使用量
    MemoryUsage,
    /// 网络吞吐量
    NetworkThroughput,
    /// 磁盘IO
    DiskIo,
    /// 并发数
    Concurrency,
    /// 错误率
    ErrorRate,
}

/// 指标聚合器
#[derive(Debug)]
pub struct MetricsAggregator {
    /// 聚合配置
    config: AggregationConfig,
    /// 聚合缓存
    aggregation_cache: HashMap<String, Vec<AggregatedMetrics>>,
}

/// 聚合配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    /// 聚合算法
    pub algorithm: AggregationAlgorithm,
    /// 异常值检测阈值
    pub outlier_threshold: f64,
    /// 是否移除异常值
    pub remove_outliers: bool,
}

/// 聚合算法
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregationAlgorithm {
    /// 简单统计
    Simple,
    /// 加权平均
    WeightedAverage,
    /// 时间衰减
    TimeDecay { decay_factor: f64 },
}

/// 性能报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    /// 报告生成时间
    pub generated_at: SystemTime,
    /// 报告时间范围
    pub time_range: (SystemTime, SystemTime),
    /// 任务性能摘要
    pub task_summaries: Vec<TaskPerformanceSummary>,
    /// 系统性能摘要
    pub system_summary: SystemPerformanceSummary,
    /// 性能趋势分析
    pub trend_analysis: TrendAnalysis,
    /// 性能建议
    pub recommendations: Vec<PerformanceRecommendation>,
}

/// 任务性能摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPerformanceSummary {
    /// 任务ID
    pub task_id: String,
    /// 执行时间
    pub execution_time: Duration,
    /// 平均性能指标
    pub avg_metrics: PerformanceMetrics,
    /// 性能评分
    pub performance_score: f64,
    /// 瓶颈分析
    pub bottlenecks: Vec<PerformanceBottleneck>,
}

/// 系统性能摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPerformanceSummary {
    /// 系统运行时间
    pub uptime: Duration,
    /// 平均系统负载
    pub avg_system_load: f64,
    /// 资源利用率
    pub resource_utilization: ResourceUtilization,
    /// 系统健康评分
    pub health_score: f64,
}

/// 资源利用率
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// CPU利用率
    pub cpu_utilization: f64,
    /// 内存利用率
    pub memory_utilization: f64,
    /// 网络利用率
    pub network_utilization: f64,
    /// 磁盘利用率
    pub disk_utilization: f64,
}

/// 趋势分析
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    /// 性能趋势
    pub performance_trend: TrendDirection,
    /// 趋势置信度
    pub trend_confidence: f64,
    /// 预测性能
    pub predicted_performance: Option<PerformanceMetrics>,
    /// 趋势分析时间窗口
    pub analysis_window: Duration,
}

/// 趋势方向
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    /// 性能提升
    Improving,
    /// 性能稳定
    Stable,
    /// 性能下降
    Degrading,
    /// 数据不足
    Insufficient,
}

/// 性能瓶颈
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceBottleneck {
    /// 瓶颈类型
    pub bottleneck_type: BottleneckType,
    /// 严重程度
    pub severity: BottleneckSeverity,
    /// 描述
    pub description: String,
    /// 影响程度
    pub impact_score: f64,
}

/// 瓶颈类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BottleneckType {
    /// CPU瓶颈
    Cpu,
    /// 内存瓶颈
    Memory,
    /// 网络瓶颈
    Network,
    /// 磁盘IO瓶颈
    DiskIo,
    /// 并发瓶颈
    Concurrency,
}

/// 瓶颈严重程度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BottleneckSeverity {
    /// 轻微
    Minor,
    /// 中等
    Moderate,
    /// 严重
    Severe,
    /// 关键
    Critical,
}

/// 性能建议
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceRecommendation {
    /// 建议类型
    pub recommendation_type: RecommendationType,
    /// 建议描述
    pub description: String,
    /// 预期改善程度
    pub expected_improvement: f64,
    /// 实施难度
    pub implementation_difficulty: DifficultyLevel,
}

/// 建议类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecommendationType {
    /// 配置优化
    ConfigurationOptimization,
    /// 资源调整
    ResourceAdjustment,
    /// 算法优化
    AlgorithmOptimization,
    /// 硬件升级
    HardwareUpgrade,
}

/// 实施难度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DifficultyLevel {
    /// 简单
    Easy,
    /// 中等
    Medium,
    /// 困难
    Hard,
    /// 非常困难
    VeryHard,
}

impl Default for MetricsCollectorConfig {
    fn default() -> Self {
        Self {
            collection_interval: Duration::from_secs(1),
            retention_period: Duration::from_secs(24 * 3600), // 24小时
            aggregation_windows: vec![
                Duration::from_secs(60),   // 1分钟
                Duration::from_secs(300),  // 5分钟
                Duration::from_secs(3600), // 1小时
            ],
            max_raw_metrics: 10000,
            enabled_metrics: vec![
                MetricType::CpuUsage,
                MetricType::MemoryUsage,
                MetricType::NetworkThroughput,
                MetricType::DiskIo,
                MetricType::Concurrency,
                MetricType::ErrorRate,
            ],
            enable_real_time_aggregation: true,
        }
    }
}

impl Default for AggregationConfig {
    fn default() -> Self {
        Self {
            algorithm: AggregationAlgorithm::Simple,
            outlier_threshold: 2.0, // 2个标准差
            remove_outliers: true,
        }
    }
}

impl Default for GlobalMetrics {
    fn default() -> Self {
        Self {
            system_start_time: SystemTime::now(),
            total_tasks_processed: 0,
            active_tasks_count: 0,
            total_errors: 0,
            system_avg_cpu: 0.0,
            system_avg_memory: 0.0,
            total_network_bytes: 0,
            total_disk_io_bytes: 0,
        }
    }
}

impl DefaultMetricsCollector {
    /// 创建新的指标收集器
    pub fn new(config: MetricsCollectorConfig) -> Self {
        Self {
            metrics_store: Arc::new(RwLock::new(MetricsStore {
                raw_metrics: HashMap::new(),
                aggregated_metrics: HashMap::new(),
                global_metrics: GlobalMetrics::default(),
            })),
            config,
            aggregator: Arc::new(Mutex::new(MetricsAggregator {
                config: AggregationConfig::default(),
                aggregation_cache: HashMap::new(),
            })),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// 使用默认配置创建收集器
    pub fn with_defaults() -> Self {
        Self::new(MetricsCollectorConfig::default())
    }

    /// 启动指标收集
    pub async fn start_collection(&self) -> Result<()> {
        let mut is_running = self.is_running.lock().await;
        if *is_running {
            return Ok(());
        }

        *is_running = true;

        let metrics_store = Arc::clone(&self.metrics_store);
        let aggregator = Arc::clone(&self.aggregator);
        let config = self.config.clone();
        let is_running_flag = Arc::clone(&self.is_running);

        tokio::spawn(async move {
            let mut interval = interval(config.collection_interval);

            while *is_running_flag.lock().await {
                interval.tick().await;

                // 清理过期数据
                if let Err(e) = Self::cleanup_expired_data(&metrics_store, &config).await {
                    log::error!("清理过期数据失败: {}", e);
                }

                // 实时聚合
                if config.enable_real_time_aggregation {
                    if let Err(e) =
                        Self::perform_real_time_aggregation(&metrics_store, &aggregator, &config)
                            .await
                    {
                        log::error!("实时聚合失败: {}", e);
                    }
                }
            }
        });

        log::info!("指标收集器已启动");
        Ok(())
    }

    /// 停止指标收集
    pub async fn stop_collection(&self) -> Result<()> {
        let mut is_running = self.is_running.lock().await;
        *is_running = false;

        log::info!("指标收集器已停止");
        Ok(())
    }

    /// 收集性能指标
    pub async fn collect_metrics(&self, task_id: &str, metrics: PerformanceMetrics) -> Result<()> {
        let mut store = self.metrics_store.write().await;

        // 添加到原始指标存储
        let task_metrics = store
            .raw_metrics
            .entry(task_id.to_string())
            .or_insert_with(VecDeque::new);
        task_metrics.push_back(metrics.clone());

        // 保持存储大小在限制内
        if task_metrics.len() > self.config.max_raw_metrics {
            task_metrics.pop_front();
        }

        // 更新全局指标
        store.global_metrics.total_network_bytes += metrics.network_throughput as u64;
        store.global_metrics.total_disk_io_bytes += metrics.disk_io as u64;

        // 更新系统平均值（简单移动平均）
        let task_count = store.raw_metrics.len() as f64;
        store.global_metrics.system_avg_cpu =
            (store.global_metrics.system_avg_cpu * (task_count - 1.0) + metrics.cpu_usage)
                / task_count;
        store.global_metrics.system_avg_memory = (store.global_metrics.system_avg_memory
            * (task_count - 1.0)
            + metrics.memory_usage as f64)
            / task_count;

        log::debug!(
            "收集指标: {} - CPU: {:.1}%, 内存: {}MB",
            task_id,
            metrics.cpu_usage,
            metrics.memory_usage / 1024 / 1024
        );

        Ok(())
    }

    /// 获取任务指标
    pub async fn get_task_metrics(&self, task_id: &str) -> Result<Vec<PerformanceMetrics>> {
        let store = self.metrics_store.read().await;

        if let Some(metrics) = store.raw_metrics.get(task_id) {
            Ok(metrics.iter().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取聚合指标
    pub async fn get_aggregated_metrics(
        &self,
        task_id: &str,
        window: Duration,
    ) -> Result<Option<AggregatedMetrics>> {
        let store = self.metrics_store.read().await;

        if let Some(metrics) = store.raw_metrics.get(task_id) {
            let aggregated = self.aggregate_metrics(task_id, metrics, window).await?;
            Ok(Some(aggregated))
        } else {
            Ok(None)
        }
    }

    /// 聚合指标
    async fn aggregate_metrics(
        &self,
        task_id: &str,
        metrics: &VecDeque<PerformanceMetrics>,
        window: Duration,
    ) -> Result<AggregatedMetrics> {
        let now = SystemTime::now();
        let cutoff_time = now - window;

        // 过滤时间窗口内的指标
        let windowed_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.timestamp >= cutoff_time)
            .collect();

        if windowed_metrics.is_empty() {
            return Ok(AggregatedMetrics {
                task_id: task_id.to_string(),
                time_window: window,
                cpu_stats: MetricStats::default(),
                memory_stats: MetricStats::default(),
                network_stats: MetricStats::default(),
                disk_io_stats: MetricStats::default(),
                concurrency_stats: MetricStats::default(),
                error_rate_stats: MetricStats::default(),
                aggregated_at: now,
            });
        }

        // 提取各类指标数据
        let cpu_values: Vec<f64> = windowed_metrics.iter().map(|m| m.cpu_usage).collect();
        let memory_values: Vec<f64> = windowed_metrics
            .iter()
            .map(|m| m.memory_usage as f64)
            .collect();
        let network_values: Vec<f64> = windowed_metrics
            .iter()
            .map(|m| m.network_throughput)
            .collect();
        let disk_io_values: Vec<f64> = windowed_metrics.iter().map(|m| m.disk_io).collect();
        let concurrency_values: Vec<f64> = windowed_metrics
            .iter()
            .map(|m| m.concurrent_downloads as f64)
            .collect();
        let error_rate_values: Vec<f64> = windowed_metrics.iter().map(|m| m.error_rate).collect();

        Ok(AggregatedMetrics {
            task_id: task_id.to_string(),
            time_window: window,
            cpu_stats: self.calculate_stats(&cpu_values),
            memory_stats: self.calculate_stats(&memory_values),
            network_stats: self.calculate_stats(&network_values),
            disk_io_stats: self.calculate_stats(&disk_io_values),
            concurrency_stats: self.calculate_stats(&concurrency_values),
            error_rate_stats: self.calculate_stats(&error_rate_values),
            aggregated_at: now,
        })
    }

    /// 计算统计信息
    fn calculate_stats(&self, values: &[f64]) -> MetricStats {
        if values.is_empty() {
            return MetricStats::default();
        }

        let mut sorted_values = values.to_vec();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let variance: f64 = values.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        let median = if count % 2 == 0 {
            (sorted_values[count / 2 - 1] + sorted_values[count / 2]) / 2.0
        } else {
            sorted_values[count / 2]
        };

        let p95_index = ((count as f64 * 0.95) as usize).min(count - 1);
        let p99_index = ((count as f64 * 0.99) as usize).min(count - 1);

        MetricStats {
            min: sorted_values[0],
            max: sorted_values[count - 1],
            mean,
            median,
            std_dev,
            p95: sorted_values[p95_index],
            p99: sorted_values[p99_index],
            count,
        }
    }

    /// 生成性能报告
    pub async fn generate_performance_report(
        &self,
        time_range: Option<(SystemTime, SystemTime)>,
    ) -> Result<PerformanceReport> {
        let store = self.metrics_store.read().await;

        let (start_time, end_time) = time_range.unwrap_or_else(|| {
            let now = SystemTime::now();
            let one_hour_ago = now - Duration::from_secs(3600);
            (one_hour_ago, now)
        });

        let mut task_summaries = Vec::new();

        // 为每个任务生成性能摘要
        for (task_id, metrics) in &store.raw_metrics {
            let filtered_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.timestamp >= start_time && m.timestamp <= end_time)
                .collect();

            if !filtered_metrics.is_empty() {
                let summary = self
                    .generate_task_summary(task_id, &filtered_metrics)
                    .await?;
                task_summaries.push(summary);
            }
        }

        // 生成系统性能摘要
        let system_summary = self
            .generate_system_summary(&store.global_metrics, start_time, end_time)
            .await?;

        // 生成趋势分析
        let trend_analysis = self
            .generate_trend_analysis(&store, start_time, end_time)
            .await?;

        // 生成性能建议
        let recommendations = self
            .generate_recommendations(&task_summaries, &system_summary)
            .await?;

        Ok(PerformanceReport {
            generated_at: SystemTime::now(),
            time_range: (start_time, end_time),
            task_summaries,
            system_summary,
            trend_analysis,
            recommendations,
        })
    }

    /// 生成任务性能摘要
    async fn generate_task_summary(
        &self,
        task_id: &str,
        metrics: &[&PerformanceMetrics],
    ) -> Result<TaskPerformanceSummary> {
        if metrics.is_empty() {
            return Err(anyhow::anyhow!("没有指标数据"));
        }

        let first_metric = metrics[0];
        let last_metric = metrics[metrics.len() - 1];
        let execution_time = last_metric
            .timestamp
            .duration_since(first_metric.timestamp)
            .unwrap_or(Duration::from_secs(0));

        // 计算平均指标
        let avg_cpu = metrics.iter().map(|m| m.cpu_usage).sum::<f64>() / metrics.len() as f64;
        let avg_memory = metrics.iter().map(|m| m.memory_usage).sum::<u64>() / metrics.len() as u64;
        let avg_network =
            metrics.iter().map(|m| m.network_throughput).sum::<f64>() / metrics.len() as f64;
        let avg_disk_io = metrics.iter().map(|m| m.disk_io).sum::<f64>() / metrics.len() as f64;
        let avg_concurrency = metrics
            .iter()
            .map(|m| m.concurrent_downloads)
            .sum::<usize>()
            / metrics.len();
        let avg_error_rate =
            metrics.iter().map(|m| m.error_rate).sum::<f64>() / metrics.len() as f64;

        let avg_metrics = PerformanceMetrics {
            task_id: task_id.to_string(),
            cpu_usage: avg_cpu,
            memory_usage: avg_memory,
            network_throughput: avg_network,
            disk_io: avg_disk_io,
            concurrent_downloads: avg_concurrency,
            error_rate: avg_error_rate,
            timestamp: SystemTime::now(),
        };

        // 计算性能评分（简单算法）
        let performance_score = self.calculate_performance_score(&avg_metrics);

        // 分析瓶颈
        let bottlenecks = self.analyze_bottlenecks(&avg_metrics);

        Ok(TaskPerformanceSummary {
            task_id: task_id.to_string(),
            execution_time,
            avg_metrics,
            performance_score,
            bottlenecks,
        })
    }

    /// 计算性能评分
    fn calculate_performance_score(&self, metrics: &PerformanceMetrics) -> f64 {
        // 简单的性能评分算法
        let cpu_score = (100.0 - metrics.cpu_usage).max(0.0);
        let memory_score = if metrics.memory_usage > 0 {
            (100.0 - (metrics.memory_usage as f64 / (8_u64 * 1024 * 1024 * 1024) as f64 * 100.0))
                .max(0.0)
        } else {
            100.0
        };
        let network_score = (metrics.network_throughput / 1024.0 / 1024.0).min(100.0); // MB/s
        let error_score = (100.0 - metrics.error_rate * 100.0).max(0.0);

        (cpu_score + memory_score + network_score + error_score) / 4.0
    }

    /// 分析性能瓶颈
    fn analyze_bottlenecks(&self, metrics: &PerformanceMetrics) -> Vec<PerformanceBottleneck> {
        let mut bottlenecks = Vec::new();

        // CPU瓶颈检测
        if metrics.cpu_usage > 90.0 {
            bottlenecks.push(PerformanceBottleneck {
                bottleneck_type: BottleneckType::Cpu,
                severity: BottleneckSeverity::Critical,
                description: format!("CPU使用率过高: {:.1}%", metrics.cpu_usage),
                impact_score: metrics.cpu_usage / 100.0,
            });
        } else if metrics.cpu_usage > 80.0 {
            bottlenecks.push(PerformanceBottleneck {
                bottleneck_type: BottleneckType::Cpu,
                severity: BottleneckSeverity::Severe,
                description: format!("CPU使用率较高: {:.1}%", metrics.cpu_usage),
                impact_score: metrics.cpu_usage / 100.0,
            });
        }

        // 内存瓶颈检测
        let memory_gb = metrics.memory_usage as f64 / 1024.0 / 1024.0 / 1024.0;
        if memory_gb > 14.0 {
            bottlenecks.push(PerformanceBottleneck {
                bottleneck_type: BottleneckType::Memory,
                severity: BottleneckSeverity::Critical,
                description: format!("内存使用量过高: {:.1}GB", memory_gb),
                impact_score: (memory_gb / 16.0).min(1.0),
            });
        }

        // 网络瓶颈检测
        let network_mbps = metrics.network_throughput / 1024.0 / 1024.0;
        if network_mbps < 1.0 && metrics.concurrent_downloads > 0 {
            bottlenecks.push(PerformanceBottleneck {
                bottleneck_type: BottleneckType::Network,
                severity: BottleneckSeverity::Moderate,
                description: format!("网络吞吐量较低: {:.1}MB/s", network_mbps),
                impact_score: 1.0 - (network_mbps / 10.0).min(1.0),
            });
        }

        // 错误率瓶颈检测
        if metrics.error_rate > 0.1 {
            bottlenecks.push(PerformanceBottleneck {
                bottleneck_type: BottleneckType::Network,
                severity: BottleneckSeverity::Severe,
                description: format!("错误率过高: {:.1}%", metrics.error_rate * 100.0),
                impact_score: metrics.error_rate,
            });
        }

        bottlenecks
    }

    /// 生成系统性能摘要
    async fn generate_system_summary(
        &self,
        global_metrics: &GlobalMetrics,
        start_time: SystemTime,
        end_time: SystemTime,
    ) -> Result<SystemPerformanceSummary> {
        let uptime = SystemTime::now()
            .duration_since(global_metrics.system_start_time)
            .unwrap_or(Duration::from_secs(0));

        let resource_utilization = ResourceUtilization {
            cpu_utilization: global_metrics.system_avg_cpu,
            memory_utilization: global_metrics.system_avg_memory,
            network_utilization: (global_metrics.total_network_bytes as f64
                / 1024.0
                / 1024.0
                / 1024.0)
                .min(100.0),
            disk_utilization: (global_metrics.total_disk_io_bytes as f64
                / 1024.0
                / 1024.0
                / 1024.0)
                .min(100.0),
        };

        // 计算系统健康评分
        let health_score = ((100.0 - resource_utilization.cpu_utilization).max(0.0)
            + (100.0 - resource_utilization.memory_utilization).max(0.0)
            + resource_utilization.network_utilization
            + resource_utilization.disk_utilization)
            / 4.0;

        Ok(SystemPerformanceSummary {
            uptime,
            avg_system_load: global_metrics.system_avg_cpu / 100.0,
            resource_utilization,
            health_score,
        })
    }

    /// 生成趋势分析
    async fn generate_trend_analysis(
        &self,
        store: &MetricsStore,
        start_time: SystemTime,
        end_time: SystemTime,
    ) -> Result<TrendAnalysis> {
        // 简化的趋势分析实现
        Ok(TrendAnalysis {
            performance_trend: TrendDirection::Stable,
            trend_confidence: 0.8,
            predicted_performance: None,
            analysis_window: end_time
                .duration_since(start_time)
                .unwrap_or(Duration::from_secs(3600)),
        })
    }

    /// 生成性能建议
    async fn generate_recommendations(
        &self,
        task_summaries: &[TaskPerformanceSummary],
        system_summary: &SystemPerformanceSummary,
    ) -> Result<Vec<PerformanceRecommendation>> {
        let mut recommendations = Vec::new();

        // 基于系统资源利用率生成建议
        if system_summary.resource_utilization.cpu_utilization > 80.0 {
            recommendations.push(PerformanceRecommendation {
                recommendation_type: RecommendationType::ConfigurationOptimization,
                description: "考虑降低并发下载数量以减少CPU负载".to_string(),
                expected_improvement: 20.0,
                implementation_difficulty: DifficultyLevel::Easy,
            });
        }

        if system_summary.resource_utilization.memory_utilization > 80.0 {
            recommendations.push(PerformanceRecommendation {
                recommendation_type: RecommendationType::ResourceAdjustment,
                description: "考虑增加系统内存或优化内存使用".to_string(),
                expected_improvement: 30.0,
                implementation_difficulty: DifficultyLevel::Medium,
            });
        }

        // 基于任务性能生成建议
        let avg_performance_score: f64 = task_summaries
            .iter()
            .map(|s| s.performance_score)
            .sum::<f64>()
            / task_summaries.len().max(1) as f64;

        if avg_performance_score < 60.0 {
            recommendations.push(PerformanceRecommendation {
                recommendation_type: RecommendationType::AlgorithmOptimization,
                description: "考虑优化下载算法和重试策略".to_string(),
                expected_improvement: 25.0,
                implementation_difficulty: DifficultyLevel::Hard,
            });
        }

        Ok(recommendations)
    }

    /// 清理过期数据
    async fn cleanup_expired_data(
        metrics_store: &Arc<RwLock<MetricsStore>>,
        config: &MetricsCollectorConfig,
    ) -> Result<()> {
        let mut store = metrics_store.write().await;
        let cutoff_time = SystemTime::now() - config.retention_period;

        for (_, metrics) in store.raw_metrics.iter_mut() {
            while let Some(front) = metrics.front() {
                if front.timestamp < cutoff_time {
                    metrics.pop_front();
                } else {
                    break;
                }
            }
        }

        // 移除空的任务指标
        store.raw_metrics.retain(|_, metrics| !metrics.is_empty());

        Ok(())
    }

    /// 执行实时聚合
    async fn perform_real_time_aggregation(
        metrics_store: &Arc<RwLock<MetricsStore>>,
        aggregator: &Arc<Mutex<MetricsAggregator>>,
        config: &MetricsCollectorConfig,
    ) -> Result<()> {
        let store = metrics_store.read().await;
        let mut agg = aggregator.lock().await;

        for (task_id, _) in &store.raw_metrics {
            for &window in &config.aggregation_windows {
                // 这里可以实现更复杂的实时聚合逻辑
                // 目前简化处理
            }
        }

        Ok(())
    }

    /// 获取全局指标
    pub async fn get_global_metrics(&self) -> GlobalMetrics {
        let store = self.metrics_store.read().await;
        store.global_metrics.clone()
    }
}

impl Default for MetricStats {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
            p95: 0.0,
            p99: 0.0,
            count: 0,
        }
    }
}

/// 指标收集器trait，用于依赖注入
#[async_trait]
pub trait MetricsCollector: Send + Sync {
    /// 收集性能指标
    async fn collect_metrics(&self, task_id: &str, metrics: PerformanceMetrics) -> Result<()>;

    /// 获取任务指标
    async fn get_task_metrics(&self, task_id: &str) -> Result<Vec<PerformanceMetrics>>;

    /// 获取聚合指标
    async fn get_aggregated_metrics(
        &self,
        task_id: &str,
        window: Duration,
    ) -> Result<Option<AggregatedMetrics>>;

    /// 生成性能报告
    async fn generate_performance_report(
        &self,
        time_range: Option<(SystemTime, SystemTime)>,
    ) -> Result<PerformanceReport>;

    /// 启动收集
    async fn start_collection(&self) -> Result<()>;

    /// 停止收集
    async fn stop_collection(&self) -> Result<()>;
}

#[async_trait]
impl MetricsCollector for DefaultMetricsCollector {
    async fn collect_metrics(&self, task_id: &str, metrics: PerformanceMetrics) -> Result<()> {
        self.collect_metrics(task_id, metrics).await
    }

    async fn get_task_metrics(&self, task_id: &str) -> Result<Vec<PerformanceMetrics>> {
        self.get_task_metrics(task_id).await
    }

    async fn get_aggregated_metrics(
        &self,
        task_id: &str,
        window: Duration,
    ) -> Result<Option<AggregatedMetrics>> {
        self.get_aggregated_metrics(task_id, window).await
    }

    async fn generate_performance_report(
        &self,
        time_range: Option<(SystemTime, SystemTime)>,
    ) -> Result<PerformanceReport> {
        self.generate_performance_report(time_range).await
    }

    async fn start_collection(&self) -> Result<()> {
        self.start_collection().await
    }

    async fn stop_collection(&self) -> Result<()> {
        self.stop_collection().await
    }
}
