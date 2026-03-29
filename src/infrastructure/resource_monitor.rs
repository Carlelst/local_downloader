/// 系统资源监控模块
///
/// 该模块负责监控系统资源使用情况，包括磁盘空间、内存使用等，
/// 并在资源不足时提供预警和自动处理功能。
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::time::interval;

/// 系统资源监控器
pub struct SystemResourceMonitor {
    /// 监控配置
    config: ResourceMonitorConfig,
    /// 资源使用历史
    resource_history: Vec<ResourceSnapshot>,
    /// 预警阈值
    thresholds: ResourceThresholds,
    /// 清理策略
    cleanup_policies: Vec<CleanupPolicy>,
}

/// 资源监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMonitorConfig {
    /// 监控间隔
    pub monitor_interval: Duration,
    /// 历史记录保留时间
    pub history_retention: Duration,
    /// 启用的监控项
    pub enabled_monitors: Vec<MonitorType>,
    /// 临时文件目录
    pub temp_directories: Vec<PathBuf>,
}

/// 监控类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitorType {
    /// 磁盘空间监控
    DiskSpace,
    /// 内存使用监控
    Memory,
    /// CPU使用监控
    Cpu,
    /// 网络使用监控
    Network,
}

/// 资源快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// 快照时间
    pub timestamp: SystemTime,
    /// 磁盘使用情况
    pub disk_usage: HashMap<PathBuf, DiskUsage>,
    /// 内存使用情况
    pub memory_usage: MemoryUsage,
    /// CPU使用情况
    pub cpu_usage: CpuUsage,
}

/// 磁盘使用情况
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskUsage {
    /// 总空间（字节）
    pub total_bytes: u64,
    /// 已使用空间（字节）
    pub used_bytes: u64,
    /// 可用空间（字节）
    pub available_bytes: u64,
    /// 使用率（百分比）
    pub usage_percentage: f64,
}
/// 内存使用情况
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    /// 总内存（字节）
    pub total_bytes: u64,
    /// 已使用内存（字节）
    pub used_bytes: u64,
    /// 可用内存（字节）
    pub available_bytes: u64,
    /// 使用率（百分比）
    pub usage_percentage: f64,
}

/// CPU使用情况
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuUsage {
    /// CPU使用率（百分比）
    pub usage_percentage: f64,
    /// 负载平均值
    pub load_average: [f64; 3], // 1分钟、5分钟、15分钟
}

/// 资源阈值配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceThresholds {
    /// 磁盘空间阈值
    pub disk_space_warning: f64, // 警告阈值（百分比）
    pub disk_space_critical: f64, // 严重阈值（百分比）
    pub disk_space_min_free: u64, // 最小可用空间（字节）

    /// 内存使用阈值
    pub memory_warning: f64, // 警告阈值（百分比）
    pub memory_critical: f64, // 严重阈值（百分比）

    /// CPU使用阈值
    pub cpu_warning: f64, // 警告阈值（百分比）
    pub cpu_critical: f64, // 严重阈值（百分比）
}

/// 清理策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPolicy {
    /// 策略名称
    pub name: String,
    /// 目标目录
    pub target_directory: PathBuf,
    /// 文件模式匹配
    pub file_pattern: String,
    /// 文件最大年龄
    pub max_age: Duration,
    /// 最大文件大小
    pub max_size: Option<u64>,
    /// 是否启用
    pub enabled: bool,
}

/// 资源预警类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResourceAlert {
    /// 磁盘空间不足
    DiskSpaceLow {
        path: PathBuf,
        available_bytes: u64,
        usage_percentage: f64,
    },
    /// 内存使用过高
    MemoryHigh {
        used_bytes: u64,
        usage_percentage: f64,
    },
    /// CPU使用过高
    CpuHigh { usage_percentage: f64 },
}

impl Default for ResourceMonitorConfig {
    fn default() -> Self {
        Self {
            monitor_interval: Duration::from_secs(30),
            history_retention: Duration::from_secs(24 * 3600),
            enabled_monitors: vec![
                MonitorType::DiskSpace,
                MonitorType::Memory,
                MonitorType::Cpu,
            ],
            temp_directories: vec![
                PathBuf::from("/tmp"),
                PathBuf::from("./temp"),
                PathBuf::from("./output"),
            ],
        }
    }
}

impl Default for ResourceThresholds {
    fn default() -> Self {
        Self {
            disk_space_warning: 80.0,
            disk_space_critical: 90.0,
            disk_space_min_free: 1024 * 1024 * 1024, // 1GB
            memory_warning: 80.0,
            memory_critical: 90.0,
            cpu_warning: 80.0,
            cpu_critical: 90.0,
        }
    }
}

impl SystemResourceMonitor {
    /// 创建新的资源监控器
    pub fn new(config: ResourceMonitorConfig, thresholds: ResourceThresholds) -> Self {
        Self {
            config,
            resource_history: Vec::new(),
            thresholds,
            cleanup_policies: Vec::new(),
        }
    }

    /// 使用默认配置创建监控器
    pub fn with_defaults() -> Self {
        Self::new(
            ResourceMonitorConfig::default(),
            ResourceThresholds::default(),
        )
    }

    /// 添加清理策略
    pub fn add_cleanup_policy(&mut self, policy: CleanupPolicy) {
        self.cleanup_policies.push(policy);
    }

    /// 启动资源监控
    pub async fn start_monitoring(&mut self) -> Result<()> {
        let mut interval = interval(self.config.monitor_interval);

        loop {
            interval.tick().await;

            // 收集资源快照
            let snapshot = self.collect_resource_snapshot().await?;

            // 检查资源阈值
            let alerts = self.check_thresholds(&snapshot);

            // 处理预警
            for alert in alerts {
                self.handle_alert(&alert).await?;
            }

            // 保存快照
            self.resource_history.push(snapshot);

            // 清理过期历史记录
            self.cleanup_history();
        }
    }

    /// 收集资源快照
    pub async fn collect_resource_snapshot(&self) -> Result<ResourceSnapshot> {
        let mut disk_usage = HashMap::new();

        // 收集磁盘使用情况
        if self
            .config
            .enabled_monitors
            .contains(&MonitorType::DiskSpace)
        {
            for dir in &self.config.temp_directories {
                // 优雅处理不存在的目录
                match self.get_disk_usage(dir).await {
                    Ok(usage) => {
                        disk_usage.insert(dir.clone(), usage);
                    }
                    Err(e) => {
                        log::debug!("跳过目录 {:?}: {}", dir, e);
                        // 继续处理其他目录，不中断整个监控过程
                    }
                }
            }
        }

        // 收集内存使用情况
        let memory_usage = if self.config.enabled_monitors.contains(&MonitorType::Memory) {
            self.get_memory_usage().await?
        } else {
            MemoryUsage {
                total_bytes: 0,
                used_bytes: 0,
                available_bytes: 0,
                usage_percentage: 0.0,
            }
        };

        // 收集CPU使用情况
        let cpu_usage = if self.config.enabled_monitors.contains(&MonitorType::Cpu) {
            self.get_cpu_usage().await?
        } else {
            CpuUsage {
                usage_percentage: 0.0,
                load_average: [0.0, 0.0, 0.0],
            }
        };

        Ok(ResourceSnapshot {
            timestamp: SystemTime::now(),
            disk_usage,
            memory_usage,
            cpu_usage,
        })
    }

    /// 获取磁盘使用情况
    async fn get_disk_usage(&self, path: &Path) -> Result<DiskUsage> {
        // 检查路径是否存在，如果不存在则优雅处理
        if !path.exists() {
            log::warn!("监控目录不存在，跳过: {:?}", path);
            return Err(anyhow::anyhow!("目录不存在: {:?}", path));
        }

        // 在实际实现中，这里会调用系统API获取磁盘使用情况
        // 为了演示，我们使用模拟数据
        let _metadata = fs::metadata(path).await?;

        // 模拟磁盘使用情况（实际实现需要使用系统调用）
        let total_bytes = 100 * 1024 * 1024 * 1024; // 100GB
        let used_bytes = 80 * 1024 * 1024 * 1024; // 80GB
        let available_bytes = total_bytes - used_bytes;
        let usage_percentage = (used_bytes as f64 / total_bytes as f64) * 100.0;

        Ok(DiskUsage {
            total_bytes,
            used_bytes,
            available_bytes,
            usage_percentage,
        })
    }

    /// 获取内存使用情况
    async fn get_memory_usage(&self) -> Result<MemoryUsage> {
        // 在实际实现中，这里会读取 /proc/meminfo 或使用系统API
        // 为了演示，我们使用模拟数据
        let total_bytes = 16 * 1024 * 1024 * 1024; // 16GB
        let used_bytes = 8 * 1024 * 1024 * 1024; // 8GB
        let available_bytes = total_bytes - used_bytes;
        let usage_percentage = (used_bytes as f64 / total_bytes as f64) * 100.0;

        Ok(MemoryUsage {
            total_bytes,
            used_bytes,
            available_bytes,
            usage_percentage,
        })
    }

    /// 获取CPU使用情况
    async fn get_cpu_usage(&self) -> Result<CpuUsage> {
        // 在实际实现中，这里会读取 /proc/stat 或使用系统API
        // 为了演示，我们使用模拟数据
        Ok(CpuUsage {
            usage_percentage: 25.0,
            load_average: [1.2, 1.5, 1.8],
        })
    }

    /// 检查资源阈值
    fn check_thresholds(&self, snapshot: &ResourceSnapshot) -> Vec<ResourceAlert> {
        let mut alerts = Vec::new();

        // 检查磁盘空间
        for (path, disk_usage) in &snapshot.disk_usage {
            if disk_usage.usage_percentage >= self.thresholds.disk_space_critical
                || disk_usage.available_bytes < self.thresholds.disk_space_min_free
            {
                alerts.push(ResourceAlert::DiskSpaceLow {
                    path: path.clone(),
                    available_bytes: disk_usage.available_bytes,
                    usage_percentage: disk_usage.usage_percentage,
                });
            }
        }

        // 检查内存使用
        if snapshot.memory_usage.usage_percentage >= self.thresholds.memory_critical {
            alerts.push(ResourceAlert::MemoryHigh {
                used_bytes: snapshot.memory_usage.used_bytes,
                usage_percentage: snapshot.memory_usage.usage_percentage,
            });
        }

        // 检查CPU使用
        if snapshot.cpu_usage.usage_percentage >= self.thresholds.cpu_critical {
            alerts.push(ResourceAlert::CpuHigh {
                usage_percentage: snapshot.cpu_usage.usage_percentage,
            });
        }

        alerts
    }

    /// 处理资源预警
    async fn handle_alert(&self, alert: &ResourceAlert) -> Result<()> {
        match alert {
            ResourceAlert::DiskSpaceLow { path, .. } => {
                log::warn!("磁盘空间不足: {:?}", path);
                self.cleanup_temp_files(path).await?;
            }
            ResourceAlert::MemoryHigh { .. } => {
                log::warn!("内存使用过高");
                // 可以触发垃圾回收或其他内存优化操作
            }
            ResourceAlert::CpuHigh { .. } => {
                log::warn!("CPU使用过高");
                // 可以降低并发级别或暂停非关键任务
            }
        }

        Ok(())
    }

    /// 清理临时文件
    async fn cleanup_temp_files(&self, path: &Path) -> Result<()> {
        for policy in &self.cleanup_policies {
            if policy.enabled && policy.target_directory == path {
                self.apply_cleanup_policy(policy).await?;
            }
        }

        Ok(())
    }

    /// 应用清理策略
    async fn apply_cleanup_policy(&self, policy: &CleanupPolicy) -> Result<()> {
        let mut dir = fs::read_dir(&policy.target_directory).await?;
        let cutoff_time = SystemTime::now() - policy.max_age;

        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;

            // 检查文件年龄
            if let Ok(modified) = metadata.modified() {
                if modified < cutoff_time {
                    // 检查文件大小（如果设置了限制）
                    if let Some(max_size) = policy.max_size {
                        if metadata.len() > max_size {
                            continue;
                        }
                    }

                    // 删除文件
                    if let Err(e) = fs::remove_file(entry.path()).await {
                        log::warn!("删除文件失败: {:?}, 错误: {}", entry.path(), e);
                    } else {
                        log::info!("清理文件: {:?}", entry.path());
                    }
                }
            }
        }

        Ok(())
    }

    /// 清理过期的历史记录
    fn cleanup_history(&mut self) {
        let cutoff_time = SystemTime::now() - self.config.history_retention;
        self.resource_history
            .retain(|snapshot| snapshot.timestamp > cutoff_time);
    }

    /// 获取当前资源使用情况
    pub async fn get_current_usage(&self) -> Result<ResourceSnapshot> {
        self.collect_resource_snapshot().await
    }

    /// 获取资源使用历史
    pub fn get_usage_history(&self) -> &[ResourceSnapshot] {
        &self.resource_history
    }

    /// 获取资源预警
    pub async fn get_current_alerts(&self) -> Result<Vec<ResourceAlert>> {
        let snapshot = self.collect_resource_snapshot().await?;
        Ok(self.check_thresholds(&snapshot))
    }

    /// 检查磁盘空间是否足够
    pub async fn check_disk_space(&self, path: &Path, required_bytes: u64) -> Result<bool> {
        let disk_usage = self.get_disk_usage(path).await?;
        Ok(disk_usage.available_bytes >= required_bytes)
    }

    /// 检查内存是否足够
    pub async fn check_memory(&self, required_bytes: u64) -> Result<bool> {
        let memory_usage = self.get_memory_usage().await?;
        Ok(memory_usage.available_bytes >= required_bytes)
    }

    /// 强制清理所有临时文件
    pub async fn force_cleanup(&self) -> Result<()> {
        for policy in &self.cleanup_policies {
            if policy.enabled {
                self.apply_cleanup_policy(policy).await?;
            }
        }

        Ok(())
    }

    /// 更新监控配置
    pub fn update_config(&mut self, config: ResourceMonitorConfig) {
        self.config = config;
    }

    /// 更新阈值配置
    pub fn update_thresholds(&mut self, thresholds: ResourceThresholds) {
        self.thresholds = thresholds;
    }
}

/// 资源监控器trait，用于依赖注入
pub trait ResourceMonitor: Send + Sync {
    /// 获取当前资源使用情况
    async fn get_current_usage(&self) -> Result<ResourceSnapshot>;

    /// 获取资源预警
    async fn get_current_alerts(&self) -> Result<Vec<ResourceAlert>>;

    /// 强制清理临时文件
    async fn force_cleanup(&self) -> Result<()>;

    /// 检查磁盘空间是否足够
    async fn check_disk_space(&self, path: &Path, required_bytes: u64) -> Result<bool>;

    /// 检查内存是否足够
    async fn check_memory(&self, required_bytes: u64) -> Result<bool>;
}

impl ResourceMonitor for SystemResourceMonitor {
    async fn get_current_usage(&self) -> Result<ResourceSnapshot> {
        self.collect_resource_snapshot().await
    }

    async fn get_current_alerts(&self) -> Result<Vec<ResourceAlert>> {
        SystemResourceMonitor::get_current_alerts(self).await
    }

    async fn force_cleanup(&self) -> Result<()> {
        SystemResourceMonitor::force_cleanup(self).await
    }

    async fn check_disk_space(&self, path: &Path, required_bytes: u64) -> Result<bool> {
        SystemResourceMonitor::check_disk_space(self, path, required_bytes).await
    }

    async fn check_memory(&self, required_bytes: u64) -> Result<bool> {
        SystemResourceMonitor::check_memory(self, required_bytes).await
    }
}
