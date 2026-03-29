/// 状态持久化系统
///
/// 该模块负责应用状态的序列化、持久化和恢复，
/// 支持崩溃恢复和状态完整性检查。
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 应用状态持久化管理器
pub struct StatePersistenceManager {
    /// 状态存储目录
    state_dir: PathBuf,
    /// 自动保存间隔
    auto_save_interval: Duration,
    /// 状态版本
    state_version: u32,
    /// 压缩配置
    compression_enabled: bool,
    /// 备份配置
    backup_config: BackupConfig,
}

/// 备份配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// 是否启用备份
    pub enabled: bool,
    /// 最大备份数量
    pub max_backups: usize,
    /// 备份间隔
    pub backup_interval: Duration,
    /// 备份目录
    pub backup_dir: PathBuf,
}

/// 应用状态快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationStateSnapshot {
    /// 快照时间戳
    pub timestamp: SystemTime,
    /// 状态版本
    pub version: u32,
    /// 下载状态
    pub download_states: HashMap<String, DownloadState>,
    /// 处理状态
    pub processing_states: HashMap<String, ProcessingState>,
    /// 配置状态
    pub configuration_state: ConfigurationState,
    /// 系统状态
    pub system_state: SystemState,
    /// 校验和
    pub checksum: String,
}

/// 下载状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    /// 任务ID
    pub task_id: String,
    /// 下载URL
    pub url: String,
    /// 总片段数
    pub total_segments: usize,
    /// 已完成片段数
    pub completed_segments: usize,
    /// 失败片段列表
    pub failed_segments: Vec<usize>,
    /// 下载进度（字节）
    pub downloaded_bytes: u64,
    /// 总大小（字节）
    pub total_bytes: u64,
    /// 任务状态
    pub status: TaskStatus,
    /// 开始时间
    pub start_time: SystemTime,
    /// 最后更新时间
    pub last_update: SystemTime,
}

/// 处理状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    /// 任务ID
    pub task_id: String,
    /// 输入文件路径
    pub input_path: PathBuf,
    /// 输出文件路径
    pub output_path: PathBuf,
    /// 处理进度（百分比）
    pub progress_percentage: f64,
    /// 处理状态
    pub status: TaskStatus,
    /// FFmpeg进程ID
    pub process_id: Option<u32>,
    /// 开始时间
    pub start_time: SystemTime,
    /// 预估完成时间
    pub estimated_completion: Option<SystemTime>,
}

/// 配置状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationState {
    /// 当前配置版本
    pub config_version: String,
    /// 配置文件路径
    pub config_file_path: PathBuf,
    /// 最后修改时间
    pub last_modified: SystemTime,
    /// 配置哈希
    pub config_hash: String,
}

/// 系统状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemState {
    /// 应用版本
    pub app_version: String,
    /// 启动时间
    pub startup_time: SystemTime,
    /// 最后活动时间
    pub last_activity: SystemTime,
    /// 运行时统计
    pub runtime_stats: RuntimeStats,
}

/// 运行时统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStats {
    /// 总下载任务数
    pub total_download_tasks: u64,
    /// 成功完成任务数
    pub completed_tasks: u64,
    /// 失败任务数
    pub failed_tasks: u64,
    /// 总下载字节数
    pub total_downloaded_bytes: u64,
    /// 总处理时间
    pub total_processing_time: Duration,
}

/// 任务状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 等待中
    Pending,
    /// 运行中
    Running,
    /// 已暂停
    Paused,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 状态恢复结果
#[derive(Debug)]
pub struct StateRecoveryResult {
    /// 是否成功恢复
    pub success: bool,
    /// 恢复的状态快照
    pub snapshot: Option<ApplicationStateSnapshot>,
    /// 恢复的任务数量
    pub recovered_tasks: usize,
    /// 错误信息
    pub errors: Vec<String>,
    /// 警告信息
    pub warnings: Vec<String>,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_backups: 5,
            backup_interval: Duration::from_secs(3600), // 1小时
            backup_dir: PathBuf::from("./backups"),
        }
    }
}

impl Default for RuntimeStats {
    fn default() -> Self {
        Self {
            total_download_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            total_downloaded_bytes: 0,
            total_processing_time: Duration::from_secs(0),
        }
    }
}

impl StatePersistenceManager {
    /// 创建新的状态持久化管理器
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            auto_save_interval: Duration::from_secs(60), // 1分钟
            state_version: 1,
            compression_enabled: true,
            backup_config: BackupConfig::default(),
        }
    }

    /// 使用自定义配置创建管理器
    pub fn with_config(
        state_dir: PathBuf,
        auto_save_interval: Duration,
        backup_config: BackupConfig,
    ) -> Self {
        Self {
            state_dir,
            auto_save_interval,
            state_version: 1,
            compression_enabled: true,
            backup_config,
        }
    }

    /// 初始化状态目录
    pub async fn initialize(&self) -> Result<()> {
        // 创建状态目录
        fs::create_dir_all(&self.state_dir)
            .await
            .context("创建状态目录失败")?;

        // 创建备份目录
        if self.backup_config.enabled {
            fs::create_dir_all(&self.backup_config.backup_dir)
                .await
                .context("创建备份目录失败")?;
        }

        Ok(())
    }

    /// 保存应用状态快照
    pub async fn save_state(&self, snapshot: &ApplicationStateSnapshot) -> Result<()> {
        let state_file = self.state_dir.join("application_state.json");
        let temp_file = self.state_dir.join("application_state.json.tmp");

        // 序列化状态
        let json_data = serde_json::to_string_pretty(snapshot).context("序列化状态失败")?;

        // 写入临时文件
        let mut file = fs::File::create(&temp_file)
            .await
            .context("创建临时状态文件失败")?;

        if self.compression_enabled {
            // 在实际实现中，这里可以添加压缩逻辑
            file.write_all(json_data.as_bytes())
                .await
                .context("写入压缩状态数据失败")?;
        } else {
            file.write_all(json_data.as_bytes())
                .await
                .context("写入状态数据失败")?;
        }

        file.sync_all().await.context("同步状态文件失败")?;

        // 原子性替换
        fs::rename(&temp_file, &state_file)
            .await
            .context("替换状态文件失败")?;

        // 创建备份
        if self.backup_config.enabled {
            self.create_backup(&state_file).await?;
        }

        Ok(())
    }

    /// 加载应用状态快照
    pub async fn load_state(&self) -> Result<Option<ApplicationStateSnapshot>> {
        let state_file = self.state_dir.join("application_state.json");

        if !state_file.exists() {
            return Ok(None);
        }

        // 读取状态文件
        let mut file = fs::File::open(&state_file)
            .await
            .context("打开状态文件失败")?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .await
            .context("读取状态文件失败")?;

        // 反序列化状态
        let snapshot: ApplicationStateSnapshot =
            serde_json::from_str(&contents).context("反序列化状态失败")?;

        // 验证状态完整性
        if self.verify_state_integrity(&snapshot).await? {
            Ok(Some(snapshot))
        } else {
            // 尝试从备份恢复
            self.restore_from_backup().await
        }
    }

    /// 验证状态完整性
    async fn verify_state_integrity(&self, snapshot: &ApplicationStateSnapshot) -> Result<bool> {
        // 计算当前校验和
        let current_checksum = self.calculate_checksum(snapshot)?;

        // 比较校验和
        Ok(current_checksum == snapshot.checksum)
    }

    /// 计算状态校验和
    fn calculate_checksum(&self, snapshot: &ApplicationStateSnapshot) -> Result<String> {
        // 创建一个不包含校验和的副本用于计算
        let mut snapshot_for_checksum = snapshot.clone();
        snapshot_for_checksum.checksum = String::new();

        let json_data = serde_json::to_string(&snapshot_for_checksum)
            .context("序列化状态用于校验和计算失败")?;

        // 使用简单的哈希算法（在实际实现中应该使用更强的算法）
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        json_data.hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
    }

    /// 创建备份
    async fn create_backup(&self, state_file: &Path) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let backup_file = self
            .backup_config
            .backup_dir
            .join(format!("application_state_{}.json", timestamp));

        fs::copy(state_file, &backup_file)
            .await
            .context("创建状态备份失败")?;

        // 清理旧备份
        self.cleanup_old_backups().await?;

        Ok(())
    }

    /// 清理旧备份
    async fn cleanup_old_backups(&self) -> Result<()> {
        let mut backups = Vec::new();
        let mut dir = fs::read_dir(&self.backup_config.backup_dir)
            .await
            .context("读取备份目录失败")?;

        while let Some(entry) = dir.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("application_state_") && name.ends_with(".json") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            backups.push((entry.path(), modified));
                        }
                    }
                }
            }
        }

        // 按修改时间排序（最新的在前）
        backups.sort_by(|a, b| b.1.cmp(&a.1));

        // 删除超出限制的备份
        for (path, _) in backups.iter().skip(self.backup_config.max_backups) {
            if let Err(e) = fs::remove_file(path).await {
                log::warn!("删除旧备份失败: {:?}, 错误: {}", path, e);
            }
        }

        Ok(())
    }

    /// 从备份恢复状态
    async fn restore_from_backup(&self) -> Result<Option<ApplicationStateSnapshot>> {
        let mut backups = Vec::new();
        let mut dir = fs::read_dir(&self.backup_config.backup_dir)
            .await
            .context("读取备份目录失败")?;

        while let Some(entry) = dir.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("application_state_") && name.ends_with(".json") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            backups.push((entry.path(), modified));
                        }
                    }
                }
            }
        }

        // 按修改时间排序（最新的在前）
        backups.sort_by(|a, b| b.1.cmp(&a.1));

        // 尝试从最新的备份恢复
        for (backup_path, _) in backups {
            match self.load_state_from_file(&backup_path).await {
                Ok(Some(snapshot)) => {
                    if self.verify_state_integrity(&snapshot).await? {
                        log::info!("从备份恢复状态成功: {:?}", backup_path);
                        return Ok(Some(snapshot));
                    }
                }
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("从备份恢复失败: {:?}, 错误: {}", backup_path, e);
                    continue;
                }
            }
        }

        Ok(None)
    }

    /// 从指定文件加载状态
    async fn load_state_from_file(
        &self,
        file_path: &Path,
    ) -> Result<Option<ApplicationStateSnapshot>> {
        if !file_path.exists() {
            return Ok(None);
        }

        let mut file = fs::File::open(file_path)
            .await
            .context("打开状态文件失败")?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .await
            .context("读取状态文件失败")?;

        let snapshot: ApplicationStateSnapshot =
            serde_json::from_str(&contents).context("反序列化状态失败")?;

        Ok(Some(snapshot))
    }

    /// 恢复应用状态
    pub async fn recover_state(&self) -> Result<StateRecoveryResult> {
        let mut result = StateRecoveryResult {
            success: false,
            snapshot: None,
            recovered_tasks: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        // 尝试加载主状态文件
        match self.load_state().await {
            Ok(Some(snapshot)) => {
                result.success = true;
                result.recovered_tasks =
                    snapshot.download_states.len() + snapshot.processing_states.len();
                result.snapshot = Some(snapshot);
            }
            Ok(None) => {
                result.warnings.push("未找到状态文件".to_string());
            }
            Err(e) => {
                result.errors.push(format!("加载状态失败: {}", e));

                // 尝试从备份恢复
                match self.restore_from_backup().await {
                    Ok(Some(snapshot)) => {
                        result.success = true;
                        result.recovered_tasks =
                            snapshot.download_states.len() + snapshot.processing_states.len();
                        result.snapshot = Some(snapshot);
                        result.warnings.push("从备份恢复状态".to_string());
                    }
                    Ok(None) => {
                        result.errors.push("未找到有效的备份文件".to_string());
                    }
                    Err(backup_error) => {
                        result
                            .errors
                            .push(format!("从备份恢复失败: {}", backup_error));
                    }
                }
            }
        }

        Ok(result)
    }

    /// 创建状态快照
    pub fn create_snapshot(
        &self,
        download_states: HashMap<String, DownloadState>,
        processing_states: HashMap<String, ProcessingState>,
        configuration_state: ConfigurationState,
        system_state: SystemState,
    ) -> Result<ApplicationStateSnapshot> {
        let mut snapshot = ApplicationStateSnapshot {
            timestamp: SystemTime::now(),
            version: self.state_version,
            download_states,
            processing_states,
            configuration_state,
            system_state,
            checksum: String::new(),
        };

        // 计算校验和
        snapshot.checksum = self.calculate_checksum(&snapshot)?;

        Ok(snapshot)
    }

    /// 启动自动保存
    pub async fn start_auto_save(
        &self,
        snapshot_provider: impl Fn() -> Result<ApplicationStateSnapshot> + Send + 'static,
    ) -> Result<()> {
        let mut interval = tokio::time::interval(self.auto_save_interval);

        loop {
            interval.tick().await;

            match snapshot_provider() {
                Ok(snapshot) => {
                    if let Err(e) = self.save_state(&snapshot).await {
                        log::error!("自动保存状态失败: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("获取状态快照失败: {}", e);
                }
            }
        }
    }

    /// 清理状态文件
    pub async fn cleanup(&self) -> Result<()> {
        let state_file = self.state_dir.join("application_state.json");
        let temp_file = self.state_dir.join("application_state.json.tmp");

        // 删除临时文件
        if temp_file.exists() {
            fs::remove_file(&temp_file)
                .await
                .context("删除临时状态文件失败")?;
        }

        // 可选：删除主状态文件
        if state_file.exists() {
            fs::remove_file(&state_file)
                .await
                .context("删除状态文件失败")?;
        }

        Ok(())
    }

    /// 获取状态统计信息
    pub async fn get_state_statistics(&self) -> Result<StateStatistics> {
        let state_file = self.state_dir.join("application_state.json");

        let mut stats = StateStatistics {
            state_file_exists: state_file.exists(),
            state_file_size: 0,
            last_modified: None,
            backup_count: 0,
            total_backup_size: 0,
        };

        if stats.state_file_exists {
            if let Ok(metadata) = fs::metadata(&state_file).await {
                stats.state_file_size = metadata.len();
                stats.last_modified = metadata.modified().ok();
            }
        }

        // 统计备份信息
        if let Ok(mut dir) = fs::read_dir(&self.backup_config.backup_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("application_state_") && name.ends_with(".json") {
                        stats.backup_count += 1;
                        if let Ok(metadata) = entry.metadata().await {
                            stats.total_backup_size += metadata.len();
                        }
                    }
                }
            }
        }

        Ok(stats)
    }
}

/// 状态统计信息
#[derive(Debug)]
pub struct StateStatistics {
    /// 状态文件是否存在
    pub state_file_exists: bool,
    /// 状态文件大小
    pub state_file_size: u64,
    /// 最后修改时间
    pub last_modified: Option<SystemTime>,
    /// 备份文件数量
    pub backup_count: usize,
    /// 备份文件总大小
    pub total_backup_size: u64,
}

/// 状态持久化trait，用于依赖注入
#[async_trait]
pub trait StatePersistence: Send + Sync {
    /// 保存状态快照
    async fn save_state(&self, snapshot: &ApplicationStateSnapshot) -> Result<()>;

    /// 加载状态快照
    async fn load_state(&self) -> Result<Option<ApplicationStateSnapshot>>;

    /// 恢复状态
    async fn recover_state(&self) -> Result<StateRecoveryResult>;

    /// 验证状态完整性
    async fn verify_integrity(&self, snapshot: &ApplicationStateSnapshot) -> Result<bool>;
}

#[async_trait]
impl StatePersistence for StatePersistenceManager {
    async fn save_state(&self, snapshot: &ApplicationStateSnapshot) -> Result<()> {
        self.save_state(snapshot).await
    }

    async fn load_state(&self) -> Result<Option<ApplicationStateSnapshot>> {
        self.load_state().await
    }

    async fn recover_state(&self) -> Result<StateRecoveryResult> {
        self.recover_state().await
    }

    async fn verify_integrity(&self, snapshot: &ApplicationStateSnapshot) -> Result<bool> {
        self.verify_state_integrity(snapshot).await
    }
}
