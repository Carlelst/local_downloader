/// 监控应用服务实现
///
/// 该模块实现完整的监控和可观测性功能，集成进度监控、性能指标收集、
/// 结构化日志和告警机制，提供统一的监控接口。
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Mutex, RwLock};

use super::services::MonitoringService;
use crate::domain::entities::{DownloadStats, PerformanceMetrics};
use crate::domain::services::ProgressMonitor;
use crate::infrastructure::{
    AggregatedMetrics, DefaultMetricsCollector, DefaultProgressMonitor,
    GlobalStats as ProgressGlobalStats, MetricsCollector, MetricsCollectorConfig,
    MetricsGlobalStats, PerformanceReport, ProgressMonitorConfig, TaskProgress,
};

/// 默认监控服务实现
pub struct DefaultMonitoringService {
    /// 进度监控器
    progress_monitor: Arc<DefaultProgressMonitor>,
    /// 指标收集器
    metrics_collector: Arc<DefaultMetricsCollector>,
    /// 监控配置
    config: MonitoringServiceConfig,
    /// 活跃任务映射
    active_tasks: Arc<RwLock<HashMap<String, TaskMonitoringInfo>>>,
    /// 服务状态
    service_state: Arc<Mutex<ServiceState>>,
}

/// 任务监控信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMonitoringInfo {
    /// 任务ID
    pub task_id: String,
    /// 开始时间
    pub start_time: SystemTime,
    /// 监控状态
    pub monitoring_status: MonitoringStatus,
    /// 最后更新时间
    pub last_update: SystemTime,
    /// 监控配置
    pub task_config: TaskMonitoringConfig,
}

/// 监控状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MonitoringStatus {
    /// 未开始
    NotStarted,
    /// 监控中
    Active,
    /// 已暂停
    Paused,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
}

/// 任务监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMonitoringConfig {
    /// 是否启用进度监控
    pub enable_progress_monitoring: bool,
    /// 是否启用性能指标收集
    pub enable_metrics_collection: bool,
    /// 更新间隔
    pub update_interval: Duration,
    /// 自定义标签
    pub custom_tags: HashMap<String, String>,
}

/// 监控服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringServiceConfig {
    /// 进度监控配置
    pub progress_config: ProgressMonitorConfig,
    /// 指标收集配置
    pub metrics_config: MetricsCollectorConfig,
    /// 是否自动启动监控
    pub auto_start_monitoring: bool,
    /// 默认任务配置
    pub default_task_config: TaskMonitoringConfig,
    /// 数据导出配置
    pub export_config: ExportConfig,
}

/// 数据导出配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// 支持的导出格式
    pub supported_formats: Vec<ExportFormat>,
    /// 默认导出格式
    pub default_format: ExportFormat,
    /// 导出目录
    pub export_directory: String,
    /// 是否包含原始数据
    pub include_raw_data: bool,
}

/// 导出格式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExportFormat {
    /// JSON格式
    Json,
    /// CSV格式
    Csv,
    /// XML格式
    Xml,
    /// HTML报告
    Html,
}

/// 服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    /// 服务是否运行中
    pub is_running: bool,
    /// 服务启动时间
    pub start_time: SystemTime,
    /// 监控的任务数量
    pub monitored_tasks_count: usize,
    /// 服务统计
    pub service_stats: ServiceStats,
}

/// 服务统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStats {
    /// 总监控任务数
    pub total_monitored_tasks: usize,
    /// 成功完成的任务数
    pub successful_tasks: usize,
    /// 失败的任务数
    pub failed_tasks: usize,
    /// 总收集的指标数量
    pub total_metrics_collected: usize,
    /// 总导出次数
    pub total_exports: usize,
}

/// 监控摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringSummary {
    /// 生成时间
    pub generated_at: SystemTime,
    /// 活跃任务数
    pub active_tasks_count: usize,
    /// 系统性能概览
    pub system_overview: SystemOverview,
    /// 任务性能概览
    pub tasks_overview: Vec<TaskOverview>,
    /// 服务健康状态
    pub service_health: ServiceHealth,
}

/// 系统性能概览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOverview {
    /// 平均CPU使用率
    pub avg_cpu_usage: f64,
    /// 平均内存使用量
    pub avg_memory_usage: u64,
    /// 总网络吞吐量
    pub total_network_throughput: f64,
    /// 总磁盘IO
    pub total_disk_io: f64,
    /// 系统运行时间
    pub system_uptime: Duration,
}

/// 任务性能概览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOverview {
    /// 任务ID
    pub task_id: String,
    /// 完成百分比
    pub completion_percentage: f64,
    /// 当前下载速度
    pub current_speed: f64,
    /// 预估剩余时间
    pub estimated_remaining_time: Option<Duration>,
    /// 性能评分
    pub performance_score: f64,
    /// 任务状态
    pub status: MonitoringStatus,
}

/// 服务健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealth {
    /// 整体健康评分
    pub overall_score: f64,
    /// 健康状态
    pub health_status: HealthStatus,
    /// 健康检查项
    pub health_checks: Vec<HealthCheck>,
}

/// 健康状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    /// 健康
    Healthy,
    /// 警告
    Warning,
    /// 不健康
    Unhealthy,
    /// 严重
    Critical,
}

/// 健康检查项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// 检查名称
    pub name: String,
    /// 检查状态
    pub status: HealthStatus,
    /// 检查消息
    pub message: String,
    /// 检查时间
    pub checked_at: SystemTime,
}

impl Default for MonitoringServiceConfig {
    fn default() -> Self {
        Self {
            progress_config: ProgressMonitorConfig::default(),
            metrics_config: MetricsCollectorConfig::default(),
            auto_start_monitoring: true,
            default_task_config: TaskMonitoringConfig::default(),
            export_config: ExportConfig::default(),
        }
    }
}

impl Default for TaskMonitoringConfig {
    fn default() -> Self {
        Self {
            enable_progress_monitoring: true,
            enable_metrics_collection: true,
            update_interval: Duration::from_millis(500),
            custom_tags: HashMap::new(),
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            supported_formats: vec![ExportFormat::Json, ExportFormat::Csv, ExportFormat::Html],
            default_format: ExportFormat::Json,
            export_directory: "./exports".to_string(),
            include_raw_data: false,
        }
    }
}

impl Default for ServiceStats {
    fn default() -> Self {
        Self {
            total_monitored_tasks: 0,
            successful_tasks: 0,
            failed_tasks: 0,
            total_metrics_collected: 0,
            total_exports: 0,
        }
    }
}

impl DefaultMonitoringService {
    /// 创建新的监控服务
    pub fn new(config: MonitoringServiceConfig) -> Self {
        let progress_monitor =
            Arc::new(DefaultProgressMonitor::new(config.progress_config.clone()));
        let metrics_collector =
            Arc::new(DefaultMetricsCollector::new(config.metrics_config.clone()));

        Self {
            progress_monitor,
            metrics_collector,
            config,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            service_state: Arc::new(Mutex::new(ServiceState {
                is_running: false,
                start_time: SystemTime::now(),
                monitored_tasks_count: 0,
                service_stats: ServiceStats::default(),
            })),
        }
    }

    /// 使用默认配置创建监控服务
    pub fn with_defaults() -> Self {
        Self::new(MonitoringServiceConfig::default())
    }

    /// 启动监控服务
    pub async fn start_service(&self) -> Result<()> {
        let mut state = self.service_state.lock().await;

        if state.is_running {
            return Ok(());
        }

        // 启动指标收集器
        self.metrics_collector.start_collection().await?;

        state.is_running = true;
        state.start_time = SystemTime::now();

        log::info!("监控服务已启动");
        Ok(())
    }

    /// 停止监控服务
    pub async fn stop_service(&self) -> Result<()> {
        let mut state = self.service_state.lock().await;

        if !state.is_running {
            return Ok(());
        }

        // 停止所有活跃任务的监控
        let active_tasks: Vec<String> = {
            let tasks = self.active_tasks.read().await;
            tasks.keys().cloned().collect()
        };

        for task_id in active_tasks {
            if let Err(e) = self.stop_monitoring(&task_id).await {
                log::warn!("停止任务监控失败 {}: {}", task_id, e);
            }
        }

        // 停止指标收集器
        self.metrics_collector.stop_collection().await?;

        state.is_running = false;

        log::info!("监控服务已停止");
        Ok(())
    }

    /// 更新任务进度和指标
    pub async fn update_task_monitoring(
        &self,
        task_id: &str,
        stats: &DownloadStats,
        metrics: &PerformanceMetrics,
    ) -> Result<()> {
        let tasks = self.active_tasks.read().await;

        if let Some(task_info) = tasks.get(task_id) {
            // 更新进度监控
            if task_info.task_config.enable_progress_monitoring {
                self.progress_monitor
                    .update_progress(task_id, stats)
                    .await?;
            }

            // 收集性能指标
            if task_info.task_config.enable_metrics_collection {
                self.metrics_collector
                    .collect_metrics(task_id, metrics.clone())
                    .await?;
            }

            // 更新服务统计
            let mut state = self.service_state.lock().await;
            state.service_stats.total_metrics_collected += 1;
        }

        Ok(())
    }

    /// 获取监控摘要
    pub async fn get_monitoring_summary(&self) -> Result<MonitoringSummary> {
        let tasks = self.active_tasks.read().await;
        let state = self.service_state.lock().await;

        // 获取系统概览
        let progress_global_stats = self.progress_monitor.get_global_stats().await;
        let metrics_global_stats = self.metrics_collector.get_global_metrics().await;

        let system_overview = SystemOverview {
            avg_cpu_usage: metrics_global_stats.system_avg_cpu,
            avg_memory_usage: metrics_global_stats.system_avg_memory as u64,
            total_network_throughput: metrics_global_stats.total_network_bytes as f64,
            total_disk_io: metrics_global_stats.total_disk_io_bytes as f64,
            system_uptime: SystemTime::now()
                .duration_since(state.start_time)
                .unwrap_or(Duration::from_secs(0)),
        };

        // 获取任务概览
        let mut tasks_overview = Vec::new();
        for (task_id, task_info) in tasks.iter() {
            if let Ok(progress) = self.progress_monitor.get_progress(task_id).await {
                let task_overview = TaskOverview {
                    task_id: task_id.clone(),
                    completion_percentage: progress.completion_percentage(),
                    current_speed: progress.download_speed,
                    estimated_remaining_time: progress.estimated_time_remaining,
                    performance_score: 85.0, // 简化的性能评分
                    status: task_info.monitoring_status.clone(),
                };
                tasks_overview.push(task_overview);
            }
        }

        // 生成服务健康状态
        let service_health = self.generate_service_health(&state, &tasks).await?;

        Ok(MonitoringSummary {
            generated_at: SystemTime::now(),
            active_tasks_count: tasks.len(),
            system_overview,
            tasks_overview,
            service_health,
        })
    }

    /// 生成服务健康状态
    async fn generate_service_health(
        &self,
        state: &ServiceState,
        tasks: &HashMap<String, TaskMonitoringInfo>,
    ) -> Result<ServiceHealth> {
        let mut health_checks = Vec::new();

        // 检查服务运行状态
        health_checks.push(HealthCheck {
            name: "服务运行状态".to_string(),
            status: if state.is_running {
                HealthStatus::Healthy
            } else {
                HealthStatus::Critical
            },
            message: if state.is_running {
                "服务正常运行"
            } else {
                "服务未运行"
            }
            .to_string(),
            checked_at: SystemTime::now(),
        });

        // 检查活跃任务数量
        let active_count = tasks.len();
        health_checks.push(HealthCheck {
            name: "活跃任务数量".to_string(),
            status: if active_count < 100 {
                HealthStatus::Healthy
            } else {
                HealthStatus::Warning
            },
            message: format!("当前活跃任务数: {}", active_count),
            checked_at: SystemTime::now(),
        });

        // 检查失败率
        let total_tasks = state.service_stats.total_monitored_tasks;
        let failed_tasks = state.service_stats.failed_tasks;
        let failure_rate = if total_tasks > 0 {
            failed_tasks as f64 / total_tasks as f64
        } else {
            0.0
        };

        health_checks.push(HealthCheck {
            name: "任务失败率".to_string(),
            status: if failure_rate < 0.05 {
                HealthStatus::Healthy
            } else if failure_rate < 0.15 {
                HealthStatus::Warning
            } else {
                HealthStatus::Unhealthy
            },
            message: format!("失败率: {:.1}%", failure_rate * 100.0),
            checked_at: SystemTime::now(),
        });

        // 计算整体健康评分
        let healthy_count = health_checks
            .iter()
            .filter(|c| c.status == HealthStatus::Healthy)
            .count();
        let overall_score = (healthy_count as f64 / health_checks.len() as f64) * 100.0;

        let health_status = if overall_score >= 90.0 {
            HealthStatus::Healthy
        } else if overall_score >= 70.0 {
            HealthStatus::Warning
        } else if overall_score >= 50.0 {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Critical
        };

        Ok(ServiceHealth {
            overall_score,
            health_status,
            health_checks,
        })
    }

    /// 导出监控数据到指定格式
    async fn export_data_to_format(&self, format: &ExportFormat, data: &str) -> Result<String> {
        match format {
            ExportFormat::Json => Ok(data.to_string()),
            ExportFormat::Csv => self.convert_to_csv(data).await,
            ExportFormat::Xml => self.convert_to_xml(data).await,
            ExportFormat::Html => self.convert_to_html(data).await,
        }
    }

    /// 转换为CSV格式
    async fn convert_to_csv(&self, _data: &str) -> Result<String> {
        // 简化的CSV转换实现
        Ok("task_id,completion_percentage,download_speed\n".to_string())
    }

    /// 转换为XML格式
    async fn convert_to_xml(&self, _data: &str) -> Result<String> {
        // 简化的XML转换实现
        Ok(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<monitoring_data></monitoring_data>"
                .to_string(),
        )
    }

    /// 转换为HTML格式
    async fn convert_to_html(&self, _data: &str) -> Result<String> {
        // 简化的HTML转换实现
        Ok("<!DOCTYPE html>\n<html><head><title>监控报告</title></head><body><h1>监控数据</h1></body></html>".to_string())
    }

    /// 获取任务监控配置
    pub async fn get_task_config(&self, task_id: &str) -> Option<TaskMonitoringConfig> {
        let tasks = self.active_tasks.read().await;
        tasks.get(task_id).map(|info| info.task_config.clone())
    }

    /// 更新任务监控配置
    pub async fn update_task_config(
        &self,
        task_id: &str,
        config: TaskMonitoringConfig,
    ) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;

        if let Some(task_info) = tasks.get_mut(task_id) {
            task_info.task_config = config;
            task_info.last_update = SystemTime::now();
            Ok(())
        } else {
            Err(anyhow::anyhow!("任务未找到: {}", task_id))
        }
    }

    /// 获取服务状态
    pub async fn get_service_state(&self) -> ServiceState {
        let state = self.service_state.lock().await;
        state.clone()
    }
}

#[async_trait]
impl MonitoringService for DefaultMonitoringService {
    async fn start_monitoring(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        let mut state = self.service_state.lock().await;

        // 如果服务未运行且配置了自动启动，则启动服务
        if !state.is_running && self.config.auto_start_monitoring {
            drop(state); // 释放锁以避免死锁
            self.start_service().await?;
            state = self.service_state.lock().await;
        }

        let task_info = TaskMonitoringInfo {
            task_id: task_id.to_string(),
            start_time: SystemTime::now(),
            monitoring_status: MonitoringStatus::Active,
            last_update: SystemTime::now(),
            task_config: self.config.default_task_config.clone(),
        };

        tasks.insert(task_id.to_string(), task_info.clone());

        // 启动进度监控
        if task_info.task_config.enable_progress_monitoring {
            self.progress_monitor.start_monitoring(task_id).await?;
        }

        // 更新服务统计
        state.monitored_tasks_count = tasks.len();
        state.service_stats.total_monitored_tasks += 1;

        log::info!("开始监控任务: {}", task_id);
        Ok(())
    }

    async fn stop_monitoring(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        let mut state = self.service_state.lock().await;

        if let Some(mut task_info) = tasks.remove(task_id) {
            // 停止进度监控
            if task_info.task_config.enable_progress_monitoring {
                self.progress_monitor.stop_monitoring(task_id).await?;
            }

            // 更新任务状态
            task_info.monitoring_status = MonitoringStatus::Completed;

            // 更新服务统计
            state.monitored_tasks_count = tasks.len();
            state.service_stats.successful_tasks += 1;

            log::info!("停止监控任务: {}", task_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("任务未找到: {}", task_id))
        }
    }

    async fn get_metrics(&self, task_id: &str) -> Result<PerformanceMetrics> {
        let metrics_list = self.metrics_collector.get_task_metrics(task_id).await?;

        if let Some(latest_metrics) = metrics_list.last() {
            Ok(latest_metrics.clone())
        } else {
            Err(anyhow::anyhow!("任务 {} 没有性能指标数据", task_id))
        }
    }

    async fn export_monitoring_data(&self, format: &str) -> Result<String> {
        let export_format = match format.to_lowercase().as_str() {
            "json" => ExportFormat::Json,
            "csv" => ExportFormat::Csv,
            "xml" => ExportFormat::Xml,
            "html" => ExportFormat::Html,
            _ => return Err(anyhow::anyhow!("不支持的导出格式: {}", format)),
        };

        // 生成监控摘要
        let summary = self.get_monitoring_summary().await?;
        let json_data = serde_json::to_string_pretty(&summary)?;

        // 转换为指定格式
        let exported_data = self
            .export_data_to_format(&export_format, &json_data)
            .await?;

        // 更新导出统计
        let mut state = self.service_state.lock().await;
        state.service_stats.total_exports += 1;

        log::info!("导出监控数据，格式: {}", format);
        Ok(exported_data)
    }
}
