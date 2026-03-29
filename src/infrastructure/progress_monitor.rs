/// 进度监控器实现
///
/// 该模块实现实时进度跟踪和统计功能，支持多任务并发监控，
/// 提供准确的进度更新和完成时间预估。
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Instant};

use crate::domain::entities::DownloadStats;
use crate::domain::services::ProgressMonitor;

/// 默认进度监控器实现
pub struct DefaultProgressMonitor {
    /// 活跃任务映射
    active_tasks: Arc<RwLock<HashMap<String, TaskProgress>>>,
    /// 监控配置
    config: ProgressMonitorConfig,
    /// 统计收集器
    stats_collector: Arc<Mutex<StatsCollector>>,
}

/// 任务进度信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    /// 任务ID
    pub task_id: String,
    /// 当前下载统计
    pub current_stats: DownloadStats,
    /// 历史统计点
    pub history: Vec<ProgressSnapshot>,
    /// 开始时间
    pub start_time: SystemTime,
    /// 最后更新时间
    pub last_update: SystemTime,
    /// 预估算法状态
    pub estimation_state: EstimationState,
}

/// 进度快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    /// 快照时间
    pub timestamp: SystemTime,
    /// 已完成片段数
    pub completed_segments: usize,
    /// 已下载字节数
    pub downloaded_bytes: u64,
    /// 瞬时下载速度
    pub instant_speed: f64,
    /// 平均下载速度
    pub average_speed: f64,
}

/// 预估算法状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstimationState {
    /// 速度历史（用于平滑计算）
    pub speed_history: Vec<f64>,
    /// 趋势分析数据
    pub trend_data: TrendData,
    /// 预估准确度
    pub accuracy_score: f64,
}

/// 趋势分析数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendData {
    /// 速度趋势（正值表示加速，负值表示减速）
    pub speed_trend: f64,
    /// 趋势置信度
    pub trend_confidence: f64,
    /// 预测窗口大小
    pub prediction_window: Duration,
}

/// 进度监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressMonitorConfig {
    /// 更新间隔
    pub update_interval: Duration,
    /// 历史记录保留数量
    pub history_size: usize,
    /// 速度平滑窗口大小
    pub speed_smoothing_window: usize,
    /// 预估算法类型
    pub estimation_algorithm: EstimationAlgorithm,
    /// 是否启用趋势分析
    pub enable_trend_analysis: bool,
}

/// 预估算法类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EstimationAlgorithm {
    /// 简单平均
    SimpleAverage,
    /// 指数移动平均
    ExponentialMovingAverage { alpha: f64 },
    /// 线性回归
    LinearRegression,
    /// 自适应算法
    Adaptive,
}

/// 统计收集器
#[derive(Debug)]
pub struct StatsCollector {
    /// 全局统计
    global_stats: GlobalStats,
    /// 任务统计历史
    task_history: HashMap<String, Vec<ProgressSnapshot>>,
}

/// 全局统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStats {
    /// 总任务数
    pub total_tasks: usize,
    /// 活跃任务数
    pub active_tasks: usize,
    /// 已完成任务数
    pub completed_tasks: usize,
    /// 失败任务数
    pub failed_tasks: usize,
    /// 总下载字节数
    pub total_downloaded_bytes: u64,
    /// 平均下载速度
    pub average_download_speed: f64,
    /// 统计开始时间
    pub stats_start_time: SystemTime,
}

impl Default for ProgressMonitorConfig {
    fn default() -> Self {
        Self {
            update_interval: Duration::from_millis(500),
            history_size: 100,
            speed_smoothing_window: 10,
            estimation_algorithm: EstimationAlgorithm::Adaptive,
            enable_trend_analysis: true,
        }
    }
}

impl Default for EstimationState {
    fn default() -> Self {
        Self {
            speed_history: Vec::new(),
            trend_data: TrendData {
                speed_trend: 0.0,
                trend_confidence: 0.0,
                prediction_window: Duration::from_secs(30),
            },
            accuracy_score: 0.0,
        }
    }
}

impl Default for GlobalStats {
    fn default() -> Self {
        Self {
            total_tasks: 0,
            active_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            total_downloaded_bytes: 0,
            average_download_speed: 0.0,
            stats_start_time: SystemTime::now(),
        }
    }
}

impl DefaultProgressMonitor {
    /// 创建新的进度监控器
    pub fn new(config: ProgressMonitorConfig) -> Self {
        Self {
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            config,
            stats_collector: Arc::new(Mutex::new(StatsCollector {
                global_stats: GlobalStats::default(),
                task_history: HashMap::new(),
            })),
        }
    }

    /// 使用默认配置创建监控器
    pub fn with_defaults() -> Self {
        Self::new(ProgressMonitorConfig::default())
    }

    /// 计算预估完成时间
    fn calculate_estimated_time(&self, task: &TaskProgress) -> Option<Duration> {
        if task.current_stats.total_segments == 0 || task.current_stats.completed_segments == 0 {
            return None;
        }

        let remaining_segments =
            task.current_stats.total_segments - task.current_stats.completed_segments;
        if remaining_segments == 0 {
            return Some(Duration::from_secs(0));
        }

        match &self.config.estimation_algorithm {
            EstimationAlgorithm::SimpleAverage => {
                self.calculate_simple_average_estimation(task, remaining_segments)
            }
            EstimationAlgorithm::ExponentialMovingAverage { alpha } => {
                self.calculate_ema_estimation(task, remaining_segments, *alpha)
            }
            EstimationAlgorithm::LinearRegression => {
                self.calculate_linear_regression_estimation(task, remaining_segments)
            }
            EstimationAlgorithm::Adaptive => {
                self.calculate_adaptive_estimation(task, remaining_segments)
            }
        }
    }

    /// 简单平均预估
    fn calculate_simple_average_estimation(
        &self,
        task: &TaskProgress,
        remaining_segments: usize,
    ) -> Option<Duration> {
        if task.current_stats.download_speed <= 0.0 {
            return None;
        }

        let avg_bytes_per_segment = if task.current_stats.completed_segments > 0 {
            task.current_stats.downloaded_bytes as f64
                / task.current_stats.completed_segments as f64
        } else {
            return None;
        };

        let remaining_bytes = remaining_segments as f64 * avg_bytes_per_segment;
        let estimated_seconds = remaining_bytes / task.current_stats.download_speed;

        Some(Duration::from_secs_f64(estimated_seconds))
    }

    /// 指数移动平均预估
    fn calculate_ema_estimation(
        &self,
        task: &TaskProgress,
        remaining_segments: usize,
        alpha: f64,
    ) -> Option<Duration> {
        if task.estimation_state.speed_history.is_empty() {
            return self.calculate_simple_average_estimation(task, remaining_segments);
        }

        // 计算EMA速度
        let mut ema_speed = task.estimation_state.speed_history[0];
        for &speed in task.estimation_state.speed_history.iter().skip(1) {
            ema_speed = alpha * speed + (1.0 - alpha) * ema_speed;
        }

        if ema_speed <= 0.0 {
            return None;
        }

        let avg_bytes_per_segment = if task.current_stats.completed_segments > 0 {
            task.current_stats.downloaded_bytes as f64
                / task.current_stats.completed_segments as f64
        } else {
            return None;
        };

        let remaining_bytes = remaining_segments as f64 * avg_bytes_per_segment;
        let estimated_seconds = remaining_bytes / ema_speed;

        Some(Duration::from_secs_f64(estimated_seconds))
    }

    /// 线性回归预估
    fn calculate_linear_regression_estimation(
        &self,
        task: &TaskProgress,
        remaining_segments: usize,
    ) -> Option<Duration> {
        if task.history.len() < 3 {
            return self.calculate_simple_average_estimation(task, remaining_segments);
        }

        // 使用最近的历史数据进行线性回归
        let recent_history: Vec<_> = task
            .history
            .iter()
            .rev()
            .take(self.config.speed_smoothing_window)
            .collect();

        if recent_history.len() < 2 {
            return None;
        }

        // 计算线性回归参数
        let n = recent_history.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        let start_time = recent_history.last().unwrap().timestamp;

        for (i, snapshot) in recent_history.iter().enumerate() {
            let x = i as f64;
            let y = snapshot.completed_segments as f64;

            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);
        let intercept = (sum_y - slope * sum_x) / n;

        if slope <= 0.0 {
            return None;
        }

        // 预测完成时间
        let target_segments = task.current_stats.total_segments as f64;
        let current_segments = task.current_stats.completed_segments as f64;
        let remaining_time_points =
            (target_segments - intercept) / slope - (current_segments - intercept) / slope;

        // 转换为实际时间（假设每个时间点对应更新间隔）
        let estimated_duration = Duration::from_secs_f64(
            remaining_time_points * self.config.update_interval.as_secs_f64(),
        );

        Some(estimated_duration)
    }

    /// 自适应预估算法
    fn calculate_adaptive_estimation(
        &self,
        task: &TaskProgress,
        remaining_segments: usize,
    ) -> Option<Duration> {
        // 根据历史数据的质量选择最佳算法
        if task.history.len() < 5 {
            return self.calculate_simple_average_estimation(task, remaining_segments);
        }

        // 计算速度变化的方差来判断稳定性
        let recent_speeds: Vec<f64> = task
            .history
            .iter()
            .rev()
            .take(self.config.speed_smoothing_window)
            .map(|s| s.average_speed)
            .collect();

        if recent_speeds.is_empty() {
            return None;
        }

        let mean_speed: f64 = recent_speeds.iter().sum::<f64>() / recent_speeds.len() as f64;
        let variance: f64 = recent_speeds
            .iter()
            .map(|&speed| (speed - mean_speed).powi(2))
            .sum::<f64>()
            / recent_speeds.len() as f64;

        let coefficient_of_variation = if mean_speed > 0.0 {
            variance.sqrt() / mean_speed
        } else {
            f64::INFINITY
        };

        // 根据变异系数选择算法
        if coefficient_of_variation < 0.1 {
            // 速度稳定，使用简单平均
            self.calculate_simple_average_estimation(task, remaining_segments)
        } else if coefficient_of_variation < 0.3 {
            // 速度有一定波动，使用EMA
            self.calculate_ema_estimation(task, remaining_segments, 0.3)
        } else {
            // 速度波动较大，使用线性回归
            self.calculate_linear_regression_estimation(task, remaining_segments)
        }
    }

    /// 更新速度历史
    fn update_speed_history(&self, task: &mut TaskProgress, current_speed: f64) {
        task.estimation_state.speed_history.push(current_speed);

        // 保持历史记录在合理范围内
        if task.estimation_state.speed_history.len() > self.config.speed_smoothing_window * 2 {
            task.estimation_state
                .speed_history
                .drain(0..self.config.speed_smoothing_window);
        }

        // 更新趋势分析
        if self.config.enable_trend_analysis {
            self.update_trend_analysis(task);
        }
    }

    /// 更新趋势分析
    fn update_trend_analysis(&self, task: &mut TaskProgress) {
        let history = &task.estimation_state.speed_history;
        if history.len() < 3 {
            return;
        }

        // 计算速度趋势（简单线性回归的斜率）
        let n = history.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, &speed) in history.iter().enumerate() {
            let x = i as f64;
            let y = speed;

            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);

        // 计算相关系数作为置信度
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;

        let mut ss_xx = 0.0;
        let mut ss_yy = 0.0;
        let mut ss_xy = 0.0;

        for (i, &speed) in history.iter().enumerate() {
            let x = i as f64;
            let y = speed;

            ss_xx += (x - mean_x).powi(2);
            ss_yy += (y - mean_y).powi(2);
            ss_xy += (x - mean_x) * (y - mean_y);
        }

        let correlation = if ss_xx > 0.0 && ss_yy > 0.0 {
            ss_xy / (ss_xx * ss_yy).sqrt()
        } else {
            0.0
        };

        task.estimation_state.trend_data.speed_trend = slope;
        task.estimation_state.trend_data.trend_confidence = correlation.abs();
    }

    /// 创建进度快照
    fn create_progress_snapshot(&self, stats: &DownloadStats) -> ProgressSnapshot {
        ProgressSnapshot {
            timestamp: SystemTime::now(),
            completed_segments: stats.completed_segments,
            downloaded_bytes: stats.downloaded_bytes,
            instant_speed: stats.download_speed,
            average_speed: stats.download_speed, // 这里可以计算更精确的平均速度
        }
    }

    /// 获取全局统计信息
    pub async fn get_global_stats(&self) -> GlobalStats {
        let collector = self.stats_collector.lock().await;
        collector.global_stats.clone()
    }

    /// 获取任务历史
    pub async fn get_task_history(&self, task_id: &str) -> Option<Vec<ProgressSnapshot>> {
        let collector = self.stats_collector.lock().await;
        collector.task_history.get(task_id).cloned()
    }

    /// 清理已完成任务的历史数据
    pub async fn cleanup_completed_tasks(&self) -> Result<()> {
        let mut collector = self.stats_collector.lock().await;
        let active_tasks = self.active_tasks.read().await;

        // 保留活跃任务的历史数据
        collector
            .task_history
            .retain(|task_id, _| active_tasks.contains_key(task_id));

        Ok(())
    }
}

#[async_trait]
impl ProgressMonitor for DefaultProgressMonitor {
    async fn start_monitoring(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        let mut collector = self.stats_collector.lock().await;

        let task_progress = TaskProgress {
            task_id: task_id.to_string(),
            current_stats: DownloadStats::default(),
            history: Vec::new(),
            start_time: SystemTime::now(),
            last_update: SystemTime::now(),
            estimation_state: EstimationState::default(),
        };

        tasks.insert(task_id.to_string(), task_progress);

        // 更新全局统计
        collector.global_stats.total_tasks += 1;
        collector.global_stats.active_tasks += 1;

        log::info!("开始监控任务: {}", task_id);
        Ok(())
    }

    async fn update_progress(&self, task_id: &str, stats: &DownloadStats) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        let mut collector = self.stats_collector.lock().await;

        if let Some(task) = tasks.get_mut(task_id) {
            // 创建进度快照
            let snapshot = self.create_progress_snapshot(stats);

            // 更新速度历史
            let mut updated_task = task.clone();
            self.update_speed_history(&mut updated_task, stats.download_speed);

            // 计算预估完成时间
            let estimated_time = self.calculate_estimated_time(&updated_task);

            // 更新任务进度
            updated_task.current_stats = stats.clone();
            updated_task.last_update = SystemTime::now();

            // 如果有预估时间，更新到统计中
            if let Some(est_time) = estimated_time {
                updated_task.current_stats.estimated_time_remaining = Some(est_time);
            }

            // 添加到历史记录
            updated_task.history.push(snapshot.clone());

            // 保持历史记录在合理范围内
            if updated_task.history.len() > self.config.history_size {
                updated_task
                    .history
                    .drain(0..updated_task.history.len() - self.config.history_size);
            }

            *task = updated_task;

            // 更新任务历史
            collector
                .task_history
                .entry(task_id.to_string())
                .or_insert_with(Vec::new)
                .push(snapshot);

            // 更新全局统计
            collector.global_stats.total_downloaded_bytes += stats.downloaded_bytes;

            log::debug!(
                "更新任务进度: {} - {:.1}%",
                task_id,
                stats.completion_percentage()
            );
        } else {
            return Err(anyhow::anyhow!("任务未找到: {}", task_id));
        }

        Ok(())
    }

    async fn get_progress(&self, task_id: &str) -> Result<DownloadStats> {
        let tasks = self.active_tasks.read().await;

        if let Some(task) = tasks.get(task_id) {
            Ok(task.current_stats.clone())
        } else {
            Err(anyhow::anyhow!("任务未找到: {}", task_id))
        }
    }

    async fn stop_monitoring(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        let mut collector = self.stats_collector.lock().await;

        if let Some(task) = tasks.remove(task_id) {
            // 更新全局统计
            collector.global_stats.active_tasks -= 1;

            if task.current_stats.completion_percentage() >= 100.0 {
                collector.global_stats.completed_tasks += 1;
            } else {
                collector.global_stats.failed_tasks += 1;
            }

            log::info!("停止监控任务: {}", task_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("任务未找到: {}", task_id))
        }
    }

    async fn get_active_tasks(&self) -> Result<Vec<String>> {
        let tasks = self.active_tasks.read().await;
        Ok(tasks.keys().cloned().collect())
    }
}
