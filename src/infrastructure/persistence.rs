/// 数据持久化基础设施
/// 提供数据存储和检索的具体实现
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 持久化存储trait - 使用JSON值以支持trait对象
#[async_trait]
pub trait PersistenceStore: Send + Sync {
    /// 保存JSON数据
    async fn save_json(&self, key: &str, data: &serde_json::Value) -> Result<()>;

    /// 加载JSON数据
    async fn load_json(&self, key: &str) -> Result<Option<serde_json::Value>>;

    /// 删除数据
    async fn delete(&self, key: &str) -> Result<()>;

    /// 检查键是否存在
    async fn exists(&self, key: &str) -> Result<bool>;

    /// 列出所有键
    async fn list_keys(&self) -> Result<Vec<String>>;

    /// 清空存储
    async fn clear(&self) -> Result<()>;
}

/// 持久化存储扩展trait - 提供泛型方法
pub trait PersistenceStoreExt: PersistenceStore {
    /// 保存数据
    async fn save<T: Serialize + Send + Sync>(&self, key: &str, data: &T) -> Result<()> {
        let json_value = serde_json::to_value(data)?;
        self.save_json(key, &json_value).await
    }

    /// 加载数据
    async fn load<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        if let Some(json_value) = self.load_json(key).await? {
            let data: T = serde_json::from_value(json_value)?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }
}

/// JSON文件持久化实现
pub struct JsonFilePersistence {
    base_dir: std::path::PathBuf,
    filesystem: Box<dyn crate::infrastructure::FileSystem>,
}

impl JsonFilePersistence {
    pub fn new(
        base_dir: std::path::PathBuf,
        filesystem: Box<dyn crate::infrastructure::FileSystem>,
    ) -> Self {
        Self {
            base_dir,
            filesystem,
        }
    }

    fn key_to_path(&self, key: &str) -> std::path::PathBuf {
        // 将键转换为安全的文件名
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.base_dir.join(format!("{}.json", safe_key))
    }
}

#[async_trait]
impl PersistenceStore for JsonFilePersistence {
    async fn save_json(&self, key: &str, data: &serde_json::Value) -> Result<()> {
        let path = self.key_to_path(key);
        let json_data = serde_json::to_vec_pretty(data)?;
        self.filesystem.write_file(&path, &json_data).await?;
        Ok(())
    }

    async fn load_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let path = self.key_to_path(key);

        if !self.filesystem.file_exists(&path).await {
            return Ok(None);
        }

        let json_data = self.filesystem.read_file(&path).await?;
        let data: serde_json::Value = serde_json::from_slice(&json_data)?;
        Ok(Some(data))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.key_to_path(key);
        if self.filesystem.file_exists(&path).await {
            self.filesystem.remove_file(&path).await?;
        }
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let path = self.key_to_path(key);
        Ok(self.filesystem.file_exists(&path).await)
    }

    async fn list_keys(&self) -> Result<Vec<String>> {
        if !self.filesystem.file_exists(&self.base_dir).await {
            return Ok(Vec::new());
        }

        let entries = self.filesystem.list_dir(&self.base_dir).await?;
        let mut keys = Vec::new();

        for entry in entries {
            if let Some(filename) = entry.file_name() {
                if let Some(filename_str) = filename.to_str() {
                    if filename_str.ends_with(".json") {
                        let key = filename_str.trim_end_matches(".json").to_string();
                        keys.push(key);
                    }
                }
            }
        }

        Ok(keys)
    }

    async fn clear(&self) -> Result<()> {
        if self.filesystem.file_exists(&self.base_dir).await {
            self.filesystem.remove_dir_all(&self.base_dir).await?;
        }
        self.filesystem.create_dir_all(&self.base_dir).await?;
        Ok(())
    }
}

/// 内存持久化实现（用于测试）
pub struct InMemoryPersistence {
    data: tokio::sync::RwLock<HashMap<String, serde_json::Value>>,
}

impl InMemoryPersistence {
    pub fn new() -> Self {
        Self {
            data: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl PersistenceStore for InMemoryPersistence {
    async fn save_json(&self, key: &str, data: &serde_json::Value) -> Result<()> {
        let mut store = self.data.write().await;
        store.insert(key.to_string(), data.clone());
        Ok(())
    }

    async fn load_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let store = self.data.read().await;
        Ok(store.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut store = self.data.write().await;
        store.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let store = self.data.read().await;
        Ok(store.contains_key(key))
    }

    async fn list_keys(&self) -> Result<Vec<String>> {
        let store = self.data.read().await;
        Ok(store.keys().cloned().collect())
    }

    async fn clear(&self) -> Result<()> {
        let mut store = self.data.write().await;
        store.clear();
        Ok(())
    }
}

/// 下载进度持久化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub task_id: String,
    pub url: String,
    pub total_size: Option<u64>,
    pub downloaded_size: u64,
    pub segments_total: usize,
    pub segments_completed: usize,
    pub segments_failed: Vec<usize>,
    pub start_time: std::time::SystemTime,
    pub last_update: std::time::SystemTime,
    pub status: DownloadStatus,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// 下载状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Pending,
    InProgress,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

/// 应用状态持久化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationState {
    pub version: String,
    pub active_downloads: HashMap<String, DownloadProgress>,
    pub configuration: HashMap<String, serde_json::Value>,
    pub statistics: HashMap<String, serde_json::Value>,
    pub last_saved: std::time::SystemTime,
    pub checksum: Option<String>,
}

/// 状态管理器
pub struct StateManager {
    store: Box<dyn PersistenceStore>,
    state_key: String,
    auto_save_interval: std::time::Duration,
    integrity_check: bool,
}

impl StateManager {
    pub fn new(store: Box<dyn PersistenceStore>, state_key: String, integrity_check: bool) -> Self {
        Self {
            store,
            auto_save_interval: std::time::Duration::from_secs(30),
            state_key,
            integrity_check,
        }
    }
    /// 保存应用状态
    pub async fn save_state(&self, state: &ApplicationState) -> Result<()> {
        let mut state_to_save = state.clone();
        state_to_save.last_saved = std::time::SystemTime::now();

        // 计算校验和
        if self.integrity_check {
            let serialized = serde_json::to_string(&state_to_save)?;
            let checksum = format!("{:x}", md5::compute(serialized.as_bytes()));
            state_to_save.checksum = Some(checksum);
        }

        self.store.save(&self.state_key, &state_to_save).await?;
        Ok(())
    }

    /// 加载应用状态
    pub async fn load_state(&self) -> Result<Option<ApplicationState>> {
        if let Some(state) = self.store.load::<ApplicationState>(&self.state_key).await? {
            // 验证校验和
            if self.integrity_check {
                if let Some(stored_checksum) = &state.checksum {
                    let mut state_for_check = state.clone();
                    state_for_check.checksum = None;

                    let serialized = serde_json::to_string(&state_for_check)?;
                    let calculated_checksum = format!("{:x}", md5::compute(serialized.as_bytes()));

                    if stored_checksum != &calculated_checksum {
                        return Err(anyhow::anyhow!("状态文件校验和不匹配，可能已损坏"));
                    }
                }
            }

            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    /// 保存下载进度
    pub async fn save_download_progress(&self, progress: &DownloadProgress) -> Result<()> {
        let key = format!("download_progress_{}", progress.task_id);
        self.store.save(&key, progress).await?;
        Ok(())
    }

    /// 加载下载进度
    pub async fn load_download_progress(&self, task_id: &str) -> Result<Option<DownloadProgress>> {
        let key = format!("download_progress_{}", task_id);
        self.store.load::<DownloadProgress>(&key).await
    }

    /// 删除下载进度
    pub async fn remove_download_progress(&self, task_id: &str) -> Result<()> {
        let key = format!("download_progress_{}", task_id);
        self.store.delete(&key).await?;
        Ok(())
    }

    /// 列出所有下载进度
    pub async fn list_download_progress(&self) -> Result<Vec<DownloadProgress>> {
        let keys = self.store.list_keys().await?;
        let mut progress_list = Vec::new();

        for key in keys {
            if key.starts_with("download_progress_") {
                if let Ok(Some(progress)) = self.store.load::<DownloadProgress>(&key).await {
                    progress_list.push(progress);
                }
            }
        }

        Ok(progress_list)
    }

    /// 清理过期的下载进度
    pub async fn cleanup_expired_progress(&self, max_age: std::time::Duration) -> Result<usize> {
        let progress_list = self.list_download_progress().await?;
        let mut cleaned = 0;

        let now = std::time::SystemTime::now();
        for progress in progress_list {
            if let Ok(elapsed) = now.duration_since(progress.last_update) {
                if elapsed > max_age
                    && matches!(
                        progress.status,
                        DownloadStatus::Completed
                            | DownloadStatus::Failed(_)
                            | DownloadStatus::Cancelled
                    )
                {
                    self.remove_download_progress(&progress.task_id).await?;
                    cleaned += 1;
                }
            }
        }

        Ok(cleaned)
    }

    /// 恢复未完成的下载
    pub async fn recover_incomplete_downloads(&self) -> Result<Vec<DownloadProgress>> {
        let progress_list = self.list_download_progress().await?;
        let incomplete: Vec<_> = progress_list
            .into_iter()
            .filter(|p| {
                matches!(
                    p.status,
                    DownloadStatus::InProgress | DownloadStatus::Paused
                )
            })
            .collect();

        Ok(incomplete)
    }
}

/// 缓存管理器
pub struct CacheManager<T: PersistenceStore> {
    store: T,
    ttl: std::time::Duration,
}

impl<T: PersistenceStore> CacheManager<T> {
    pub fn new(store: T, ttl: std::time::Duration) -> Self {
        Self { store, ttl }
    }

    pub async fn get<U: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<U>> {
        let cache_key = format!("cache_{}", key);

        if let Some(cached_item) = self.store.load::<CachedItem<U>>(&cache_key).await? {
            if cached_item.is_expired() {
                self.store.delete(&cache_key).await?;
                Ok(None)
            } else {
                Ok(Some(cached_item.data))
            }
        } else {
            Ok(None)
        }
    }

    pub async fn set<U: Serialize + Send + Sync>(&self, key: &str, data: U) -> Result<()> {
        let cache_key = format!("cache_{}", key);
        let cached_item = CachedItem::new(data, self.ttl);
        self.store.save(&cache_key, &cached_item).await?;
        Ok(())
    }

    pub async fn invalidate(&self, key: &str) -> Result<()> {
        let cache_key = format!("cache_{}", key);
        self.store.delete(&cache_key).await?;
        Ok(())
    }

    pub async fn cleanup_expired(&self) -> Result<usize> {
        let keys = self.store.list_keys().await?;
        let mut cleaned = 0;

        for key in keys {
            if key.starts_with("cache_") {
                if let Ok(Some(cached_item)) =
                    self.store.load::<CachedItem<serde_json::Value>>(&key).await
                {
                    if cached_item.is_expired() {
                        self.store.delete(&key).await?;
                        cleaned += 1;
                    }
                }
            }
        }

        Ok(cleaned)
    }
}

#[derive(Serialize, Deserialize)]
struct CachedItem<T> {
    data: T,
    expires_at: std::time::SystemTime,
}

impl<T> CachedItem<T> {
    fn new(data: T, ttl: std::time::Duration) -> Self {
        Self {
            data,
            expires_at: std::time::SystemTime::now() + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        std::time::SystemTime::now() > self.expires_at
    }
}

/// 备份管理器
pub struct BackupManager {
    store: Box<dyn PersistenceStore>,
    backup_dir: std::path::PathBuf,
    max_backups: usize,
}

impl BackupManager {
    pub fn new(
        store: Box<dyn PersistenceStore>,
        backup_dir: std::path::PathBuf,
        max_backups: usize,
    ) -> Self {
        Self {
            store,
            backup_dir,
            max_backups,
        }
    }

    /// 创建备份
    pub async fn create_backup(&self, backup_name: &str) -> Result<()> {
        let keys = self.store.list_keys().await?;
        let mut backup_data = HashMap::new();

        // 收集所有数据
        for key in keys {
            if let Ok(Some(data)) = self.store.load_json(&key).await {
                backup_data.insert(key, data);
            }
        }

        // 创建备份文件
        let backup_file = self.backup_dir.join(format!("{}.backup.json", backup_name));
        let backup_content = serde_json::to_vec_pretty(&backup_data)?;

        tokio::fs::create_dir_all(&self.backup_dir).await?;
        tokio::fs::write(&backup_file, backup_content).await?;

        // 清理旧备份
        self.cleanup_old_backups().await?;

        Ok(())
    }

    /// 从备份恢复
    pub async fn restore_from_backup(&self, backup_name: &str) -> Result<()> {
        let backup_file = self.backup_dir.join(format!("{}.backup.json", backup_name));

        if !backup_file.exists() {
            return Err(anyhow::anyhow!("备份文件不存在: {}", backup_file.display()));
        }

        let backup_content = tokio::fs::read(&backup_file).await?;
        let backup_data: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&backup_content)?;

        // 清空当前存储
        self.store.clear().await?;

        // 恢复数据
        for (key, data) in backup_data {
            self.store.save_json(&key, &data).await?;
        }

        Ok(())
    }

    /// 列出可用备份
    pub async fn list_backups(&self) -> Result<Vec<String>> {
        if !self.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        let mut dir = tokio::fs::read_dir(&self.backup_dir).await?;

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if let Some(filename) = path.file_name() {
                if let Some(filename_str) = filename.to_str() {
                    if filename_str.ends_with(".backup.json") {
                        let backup_name = filename_str.trim_end_matches(".backup.json");
                        backups.push(backup_name.to_string());
                    }
                }
            }
        }

        backups.sort();
        Ok(backups)
    }

    /// 清理旧备份
    async fn cleanup_old_backups(&self) -> Result<()> {
        let mut backups = self.list_backups().await?;

        if backups.len() > self.max_backups {
            backups.sort();
            let to_remove = backups.len() - self.max_backups;

            for backup_name in backups.iter().take(to_remove) {
                let backup_file = self.backup_dir.join(format!("{}.backup.json", backup_name));
                if backup_file.exists() {
                    tokio::fs::remove_file(&backup_file).await?;
                }
            }
        }

        Ok(())
    }
}

// 为所有PersistenceStore实现扩展trait
impl<T: ?Sized + PersistenceStore> PersistenceStoreExt for T {}

impl Default for InMemoryPersistence {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DownloadStatus {
    fn default() -> Self {
        DownloadStatus::Pending
    }
}

impl Default for ApplicationState {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_downloads: HashMap::new(),
            configuration: HashMap::new(),
            statistics: HashMap::new(),
            last_saved: std::time::SystemTime::now(),
            checksum: None,
        }
    }
}
