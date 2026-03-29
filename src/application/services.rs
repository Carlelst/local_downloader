/// 应用服务模块
/// 定义高级业务操作和用例
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{watch, RwLock};
use tokio::time::interval;

use super::validation::{ConfigValidationManager, ValidationResult};
use crate::domain::entities::{Config, DownloadStats, ProcessingResult};

/// 视频处理应用服务trait
#[async_trait]
pub trait VideoProcessorService {
    /// 处理M3U8视频流
    async fn process_video_stream(
        &self,
        m3u8_url: &str,
        config: &Config,
    ) -> Result<ProcessingResult>;

    /// 获取处理进度
    async fn get_processing_progress(&self, task_id: &str) -> Result<DownloadStats>;

    /// 取消处理任务
    async fn cancel_processing(&self, task_id: &str) -> Result<()>;

    /// 清理临时文件
    async fn cleanup_temp_files(&self, config: &Config) -> Result<()>;
}

/// 配置管理器trait - 增强的配置管理功能
#[async_trait]
pub trait ConfigManager: Send + Sync {
    /// 加载配置
    async fn load_config(&self) -> Result<Config>;

    /// 验证配置
    async fn validate_config(&self, config: &Config) -> Result<()>;

    /// 重新加载配置
    async fn reload_config(&self) -> Result<Config>;

    /// 监听配置变更
    fn watch_config_changes(&self) -> watch::Receiver<Config>;

    /// 从多个源加载配置
    async fn load_from_sources(&self, sources: Vec<ConfigSource>) -> Result<Config>;

    /// 合并配置
    async fn merge_configs(&self, configs: Vec<Config>) -> Result<Config>;

    /// 获取配置模板
    async fn get_config_template(&self, template_name: &str) -> Result<Config>;

    /// 解决配置冲突
    async fn resolve_config_conflicts(&self, configs: Vec<Config>) -> Result<Config>;

    /// 启动配置热重载监听
    async fn start_hot_reload(&self, watch_sources: Vec<ConfigWatchSource>) -> Result<()>;

    /// 按优先级解决配置冲突
    async fn resolve_config_conflicts_with_priority(
        &self,
        configs: Vec<(Config, ConfigPriority)>,
    ) -> Result<Config>;
}

/// 配置源枚举
#[derive(Debug, Clone)]
pub enum ConfigSource {
    File(String),
    Environment,
    CommandLine(HashMap<String, String>),
    Default,
}

/// 配置监听源枚举
#[derive(Debug, Clone)]
pub enum ConfigWatchSource {
    File(String),
    Environment,
}

/// 配置优先级枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigPriority {
    Default = 0,     // 最低优先级
    ConfigFile = 1,  // 配置文件
    Environment = 2, // 环境变量
    CommandLine = 3, // 最高优先级
}

/// 配置管理应用服务trait
#[async_trait]
pub trait ConfigService {
    /// 加载配置
    async fn load_configuration(&self, config_path: Option<&str>) -> Result<Config>;

    /// 验证配置
    async fn validate_configuration(&self, config: &Config) -> Result<()>;

    /// 重新加载配置
    async fn reload_configuration(&self) -> Result<Config>;

    /// 合并配置源
    async fn merge_configurations(&self, configs: Vec<Config>) -> Result<Config>;
}

/// 监控应用服务trait
#[async_trait]
pub trait MonitoringService {
    /// 开始监控
    async fn start_monitoring(&self, task_id: &str) -> Result<()>;

    /// 停止监控
    async fn stop_monitoring(&self, task_id: &str) -> Result<()>;

    /// 获取性能指标
    async fn get_metrics(&self, task_id: &str) -> Result<crate::domain::PerformanceMetrics>;

    /// 导出监控数据
    async fn export_monitoring_data(&self, format: &str) -> Result<String>;
}

/// 默认配置管理器实现
#[derive(Clone)]
pub struct DefaultConfigManager {
    config_sources: Vec<ConfigSource>,
    config_sender: watch::Sender<Config>,
    config_receiver: watch::Receiver<Config>,
    config_templates: Arc<HashMap<String, Config>>,
    validator: Arc<ConfigValidationManager>,
}

impl DefaultConfigManager {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(Config::default());

        let mut templates = HashMap::new();

        // 添加预设配置模板
        templates.insert(
            "high_performance".to_string(),
            Self::create_high_performance_template(),
        );
        templates.insert(
            "low_bandwidth".to_string(),
            Self::create_low_bandwidth_template(),
        );
        templates.insert(
            "quality_focused".to_string(),
            Self::create_quality_focused_template(),
        );
        templates.insert(
            "development".to_string(),
            Self::create_development_template(),
        );

        Self {
            config_sources: vec![
                ConfigSource::Default,
                ConfigSource::File("config.yaml".to_string()),
                ConfigSource::Environment,
            ],
            config_sender: sender,
            config_receiver: receiver,
            config_templates: Arc::new(templates),
            validator: Arc::new(ConfigValidationManager::new()),
        }
    }

    pub fn with_sources(sources: Vec<ConfigSource>) -> Self {
        let (sender, receiver) = watch::channel(Config::default());

        Self {
            config_sources: sources,
            config_sender: sender,
            config_receiver: receiver,
            config_templates: Arc::new(HashMap::new()),
            validator: Arc::new(ConfigValidationManager::new()),
        }
    }

    /// 创建高性能配置模板
    fn create_high_performance_template() -> Config {
        Config {
            max_concurrent_downloads: 16,
            download_retries: 5,
            segment_timeout: 15,
            retry_interval: 1,
            buffer_size: 8 * 1024 * 1024, // 8MB
            chunk_size: 2 * 1024 * 1024,  // 2MB
            use_hw_accel: true,
            thread_count: -1,         // 使用所有可用核心
            validate_segments: false, // 跳过验证以提高速度
            ..Config::default()
        }
    }

    /// 创建低带宽配置模板
    fn create_low_bandwidth_template() -> Config {
        Config {
            max_concurrent_downloads: 2,
            download_retries: 8,
            segment_timeout: 60,
            retry_interval: 5,
            buffer_size: 1024 * 1024, // 1MB
            chunk_size: 256 * 1024,   // 256KB
            validate_segments: true,
            ..Config::default()
        }
    }

    /// 创建质量优先配置模板
    fn create_quality_focused_template() -> Config {
        Config {
            max_concurrent_downloads: 4,
            download_retries: 10,
            segment_timeout: 120,
            retry_interval: 3,
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            video_bitrate: Some("5000k".to_string()),
            audio_bitrate: Some("320k".to_string()),
            validate_segments: true,
            ..Config::default()
        }
    }

    /// 创建开发配置模板
    fn create_development_template() -> Config {
        Config {
            max_concurrent_downloads: 2,
            download_retries: 2,
            segment_timeout: 30,
            log_level: "debug".to_string(),
            validate_segments: true,
            output_directory: "./dev_output".to_string(),
            ..Config::default()
        }
    }

    /// 从文件加载配置
    async fn load_from_file(&self, file_path: &str) -> Result<Config> {
        let path = Path::new(file_path);

        if !path.exists() {
            return Err(anyhow::anyhow!("配置文件不存在: {}", file_path));
        }

        let content = tokio::fs::read_to_string(path).await?;

        let config = if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
            serde_yaml::from_str::<Config>(&content)?
        } else if file_path.ends_with(".json") {
            serde_json::from_str::<Config>(&content)?
        } else {
            return Err(anyhow::anyhow!("不支持的配置文件格式: {}", file_path));
        };

        Ok(config)
    }

    /// 从环境变量加载配置
    async fn load_from_environment(&self) -> Result<Config> {
        // 从默认配置开始，只覆盖设置了的环境变量
        let mut config = Config::default();

        // 从环境变量读取配置，覆盖默认值
        if let Ok(val) = std::env::var("VP_MAX_CONCURRENT_DOWNLOADS") {
            if let Ok(parsed) = val.parse() {
                config.max_concurrent_downloads = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_DOWNLOAD_RETRIES") {
            if let Ok(parsed) = val.parse() {
                config.download_retries = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_SEGMENT_TIMEOUT") {
            if let Ok(parsed) = val.parse() {
                config.segment_timeout = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_OUTPUT_DIRECTORY") {
            config.output_directory = val;
        }

        if let Ok(val) = std::env::var("VP_LOG_LEVEL") {
            config.log_level = val;
        }

        if let Ok(val) = std::env::var("VP_VIDEO_FORMAT") {
            config.video_format = val;
        }

        if let Ok(val) = std::env::var("VP_VIDEO_CODEC") {
            config.video_codec = val;
        }

        if let Ok(val) = std::env::var("VP_AUDIO_CODEC") {
            config.audio_codec = val;
        }

        if let Ok(val) = std::env::var("VP_USE_HW_ACCEL") {
            if let Ok(parsed) = val.parse() {
                config.use_hw_accel = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_THREAD_COUNT") {
            if let Ok(parsed) = val.parse() {
                config.thread_count = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_BUFFER_SIZE") {
            if let Ok(parsed) = val.parse() {
                config.buffer_size = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_CHUNK_SIZE") {
            if let Ok(parsed) = val.parse() {
                config.chunk_size = parsed;
            }
        }

        if let Ok(val) = std::env::var("VP_VALIDATE_SEGMENTS") {
            if let Ok(parsed) = val.parse() {
                config.validate_segments = parsed;
            }
        }

        Ok(config)
    }

    /// 从命令行参数加载配置
    async fn load_from_command_line(&self, args: &HashMap<String, String>) -> Result<Config> {
        let mut config = Config::default();

        if let Some(val) = args.get("max-concurrent-downloads") {
            config.max_concurrent_downloads =
                val.parse().unwrap_or(config.max_concurrent_downloads);
        }

        if let Some(val) = args.get("download-retries") {
            config.download_retries = val.parse().unwrap_or(config.download_retries);
        }

        if let Some(val) = args.get("segment-timeout") {
            config.segment_timeout = val.parse().unwrap_or(config.segment_timeout);
        }

        if let Some(val) = args.get("output-directory") {
            config.output_directory = val.clone();
        }

        if let Some(val) = args.get("log-level") {
            config.log_level = val.clone();
        }

        if let Some(val) = args.get("video-format") {
            config.video_format = val.clone();
        }

        if let Some(val) = args.get("video-codec") {
            config.video_codec = val.clone();
        }

        if let Some(val) = args.get("audio-codec") {
            config.audio_codec = val.clone();
        }

        if let Some(val) = args.get("use-hw-accel") {
            config.use_hw_accel = val.parse().unwrap_or(config.use_hw_accel);
        }

        if let Some(val) = args.get("thread-count") {
            config.thread_count = val.parse().unwrap_or(config.thread_count);
        }

        Ok(config)
    }

    /// 通知配置变更
    pub async fn notify_config_change(&self, config: Config) -> Result<()> {
        self.config_sender
            .send(config)
            .map_err(|_| anyhow::anyhow!("无法发送配置变更通知"))?;
        Ok(())
    }

    /// 获取详细的验证报告
    pub fn get_validation_report(&self, config: &Config) -> String {
        self.validator.get_validation_report(config)
    }

    /// 获取验证结果
    pub fn get_validation_result(&self, config: &Config) -> ValidationResult {
        self.validator.validate_config(config)
    }

    /// 获取所有验证器信息
    pub fn get_validators_info(&self) -> Vec<(String, String)> {
        self.validator.get_validators_info()
    }

    /// 快速验证（只检查错误）
    pub fn validate_quick(&self, config: &Config) -> Result<()> {
        let result = self.validator.validate_config(config);

        if !result.is_valid {
            let error_messages: Vec<String> = result
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect();
            return Err(anyhow::anyhow!(
                "配置验证失败: {}",
                error_messages.join("; ")
            ));
        }

        Ok(())
    }

    /// 启动配置文件监听
    pub async fn start_config_watching(&self, config_file: String) -> Result<()> {
        let config_manager = self.clone();
        let file_path = config_file.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5)); // 每5秒检查一次
            let mut last_modified = None;

            loop {
                interval.tick().await;

                if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                    if let Ok(modified) = metadata.modified() {
                        if last_modified.is_none() || Some(modified) != last_modified {
                            last_modified = Some(modified);

                            // 配置文件已修改，重新加载
                            if let Ok(new_config) = config_manager.load_from_file(&file_path).await
                            {
                                if let Err(e) = config_manager.validate_config(&new_config).await {
                                    log::warn!("配置文件验证失败: {}", e);
                                    continue;
                                }

                                if let Err(e) =
                                    config_manager.notify_config_change(new_config).await
                                {
                                    log::error!("配置变更通知失败: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// 启动高级配置文件监听（支持多文件和环境变量监听）
    pub async fn start_advanced_config_watching(
        &self,
        watch_sources: Vec<ConfigWatchSource>,
    ) -> Result<()> {
        let config_manager = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(2)); // 更频繁的检查
            let mut file_states: HashMap<String, std::time::SystemTime> = HashMap::new();
            let mut env_state_hash = Self::calculate_env_hash();

            loop {
                interval.tick().await;
                let mut config_changed = false;

                // 检查文件变更
                for source in &watch_sources {
                    match source {
                        ConfigWatchSource::File(file_path) => {
                            if let Ok(metadata) = tokio::fs::metadata(file_path).await {
                                if let Ok(modified) = metadata.modified() {
                                    let last_modified = file_states.get(file_path);
                                    if last_modified.is_none() || Some(&modified) != last_modified {
                                        file_states.insert(file_path.clone(), modified);
                                        config_changed = true;
                                        log::info!("检测到配置文件变更: {}", file_path);
                                    }
                                }
                            }
                        }
                        ConfigWatchSource::Environment => {
                            let current_env_hash = Self::calculate_env_hash();
                            if current_env_hash != env_state_hash {
                                env_state_hash = current_env_hash;
                                config_changed = true;
                                log::info!("检测到环境变量变更");
                            }
                        }
                    }
                }

                // 如果有变更，重新加载配置
                if config_changed {
                    match config_manager.reload_config().await {
                        Ok(new_config) => {
                            log::info!("配置热重载成功");
                            if let Err(e) = config_manager.notify_config_change(new_config).await {
                                log::error!("配置变更通知失败: {}", e);
                            }
                        }
                        Err(e) => {
                            log::error!("配置热重载失败: {}", e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// 计算环境变量哈希值（用于检测环境变量变更）
    pub fn calculate_env_hash() -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // 只监听特定的环境变量
        let env_vars = [
            "VP_MAX_CONCURRENT_DOWNLOADS",
            "VP_DOWNLOAD_RETRIES",
            "VP_SEGMENT_TIMEOUT",
            "VP_OUTPUT_DIRECTORY",
            "VP_LOG_LEVEL",
            "VP_VIDEO_FORMAT",
            "VP_VIDEO_CODEC",
            "VP_AUDIO_CODEC",
            "VP_USE_HW_ACCEL",
            "VP_THREAD_COUNT",
            "VP_BUFFER_SIZE",
            "VP_CHUNK_SIZE",
            "VP_VALIDATE_SEGMENTS",
        ];

        for var in &env_vars {
            if let Ok(value) = std::env::var(var) {
                var.hash(&mut hasher);
                value.hash(&mut hasher);
            }
        }

        hasher.finish()
    }

    /// 解决配置冲突（实现优先级处理）
    pub async fn resolve_config_conflicts_advanced(
        &self,
        configs: Vec<(Config, ConfigPriority)>,
    ) -> Result<Config> {
        if configs.is_empty() {
            return Ok(Config::default());
        }

        // 按优先级排序（高优先级在前）
        let mut sorted_configs = configs;
        sorted_configs.sort_by(|a, b| b.1.cmp(&a.1));

        let mut merged = Config::default();

        // 从低优先级到高优先级依次合并
        for (config, priority) in sorted_configs.iter().rev() {
            log::debug!("合并配置，优先级: {:?}", priority);
            merged = self
                .merge_config_with_priority(&merged, config, *priority)
                .await?;
        }

        Ok(merged)
    }

    /// 按优先级合并配置
    async fn merge_config_with_priority(
        &self,
        base: &Config,
        override_config: &Config,
        priority: ConfigPriority,
    ) -> Result<Config> {
        let mut merged = base.clone();

        // 根据优先级决定合并策略
        match priority {
            ConfigPriority::CommandLine => {
                // 命令行参数：强制覆盖所有非默认值
                self.force_merge_config(&mut merged, override_config);
            }
            ConfigPriority::Environment => {
                // 环境变量：覆盖非空值
                self.selective_merge_config(&mut merged, override_config);
            }
            ConfigPriority::ConfigFile => {
                // 配置文件：只覆盖明确设置的值
                self.careful_merge_config(&mut merged, override_config);
            }
            ConfigPriority::Default => {
                // 默认值：只填充未设置的值
                self.fill_default_config(&mut merged, override_config);
            }
        }

        Ok(merged)
    }

    /// 强制合并配置（命令行优先级）
    pub fn force_merge_config(&self, base: &mut Config, override_config: &Config) {
        // 直接覆盖所有字段
        base.max_concurrent_downloads = override_config.max_concurrent_downloads;
        base.download_retries = override_config.download_retries;
        base.segment_timeout = override_config.segment_timeout;
        base.retry_interval = override_config.retry_interval;
        base.buffer_size = override_config.buffer_size;
        base.chunk_size = override_config.chunk_size;
        base.thread_count = override_config.thread_count;
        base.use_hw_accel = override_config.use_hw_accel;
        base.validate_segments = override_config.validate_segments;

        if !override_config.output_directory.is_empty() {
            base.output_directory = override_config.output_directory.clone();
        }
        if !override_config.log_level.is_empty() {
            base.log_level = override_config.log_level.clone();
        }
        if !override_config.video_format.is_empty() {
            base.video_format = override_config.video_format.clone();
        }
        if !override_config.video_codec.is_empty() {
            base.video_codec = override_config.video_codec.clone();
        }
        if !override_config.audio_codec.is_empty() {
            base.audio_codec = override_config.audio_codec.clone();
        }
        if override_config.video_bitrate.is_some() {
            base.video_bitrate = override_config.video_bitrate.clone();
        }
        if override_config.audio_bitrate.is_some() {
            base.audio_bitrate = override_config.audio_bitrate.clone();
        }
        if !override_config.m3u8_urls.is_empty() {
            base.m3u8_urls = override_config.m3u8_urls.clone();
        }
    }

    /// 选择性合并配置（环境变量优先级）
    pub fn selective_merge_config(&self, base: &mut Config, override_config: &Config) {
        // 只覆盖明确不同于默认值的字段
        let default_config = Config::default();

        if override_config.max_concurrent_downloads != default_config.max_concurrent_downloads {
            base.max_concurrent_downloads = override_config.max_concurrent_downloads;
        }
        if override_config.download_retries != default_config.download_retries {
            base.download_retries = override_config.download_retries;
        }
        if override_config.segment_timeout != default_config.segment_timeout {
            base.segment_timeout = override_config.segment_timeout;
        }
        if override_config.retry_interval != default_config.retry_interval {
            base.retry_interval = override_config.retry_interval;
        }
        if override_config.buffer_size != default_config.buffer_size {
            base.buffer_size = override_config.buffer_size;
        }
        if override_config.chunk_size != default_config.chunk_size {
            base.chunk_size = override_config.chunk_size;
        }
        if override_config.thread_count != default_config.thread_count {
            base.thread_count = override_config.thread_count;
        }
        if override_config.use_hw_accel != default_config.use_hw_accel {
            base.use_hw_accel = override_config.use_hw_accel;
        }
        if override_config.validate_segments != default_config.validate_segments {
            base.validate_segments = override_config.validate_segments;
        }

        if override_config.output_directory != default_config.output_directory {
            base.output_directory = override_config.output_directory.clone();
        }
        if override_config.log_level != default_config.log_level {
            base.log_level = override_config.log_level.clone();
        }
        if override_config.video_format != default_config.video_format {
            base.video_format = override_config.video_format.clone();
        }
        if override_config.video_codec != default_config.video_codec {
            base.video_codec = override_config.video_codec.clone();
        }
        if override_config.audio_codec != default_config.audio_codec {
            base.audio_codec = override_config.audio_codec.clone();
        }
    }

    /// 谨慎合并配置（配置文件优先级）
    pub fn careful_merge_config(&self, base: &mut Config, override_config: &Config) {
        // 使用现有的合并逻辑，但更加保守
        if !override_config.m3u8_urls.is_empty() {
            base.m3u8_urls = override_config.m3u8_urls.clone();
        }

        // 只有当值明显不是默认值时才覆盖
        if override_config.max_concurrent_downloads > 0 {
            base.max_concurrent_downloads = override_config.max_concurrent_downloads;
        }
        if override_config.download_retries > 0 {
            base.download_retries = override_config.download_retries;
        }
        if override_config.segment_timeout > 0 {
            base.segment_timeout = override_config.segment_timeout;
        }
        if override_config.retry_interval > 0 {
            base.retry_interval = override_config.retry_interval;
        }
        if override_config.buffer_size > 0 {
            base.buffer_size = override_config.buffer_size;
        }
        if override_config.chunk_size > 0 {
            base.chunk_size = override_config.chunk_size;
        }

        if !override_config.output_directory.is_empty() {
            base.output_directory = override_config.output_directory.clone();
        }
        if !override_config.log_level.is_empty() {
            base.log_level = override_config.log_level.clone();
        }
        if !override_config.video_format.is_empty() {
            base.video_format = override_config.video_format.clone();
        }
        if !override_config.video_codec.is_empty() {
            base.video_codec = override_config.video_codec.clone();
        }
        if !override_config.audio_codec.is_empty() {
            base.audio_codec = override_config.audio_codec.clone();
        }

        // 对于布尔值和可选值，使用更智能的合并
        base.use_hw_accel = override_config.use_hw_accel;
        base.validate_segments = override_config.validate_segments;
        base.thread_count = override_config.thread_count;

        if override_config.video_bitrate.is_some() {
            base.video_bitrate = override_config.video_bitrate.clone();
        }
        if override_config.audio_bitrate.is_some() {
            base.audio_bitrate = override_config.audio_bitrate.clone();
        }
    }

    /// 填充默认配置（默认值优先级）
    pub fn fill_default_config(&self, base: &mut Config, default_config: &Config) {
        // 只填充基础配置中未设置的值
        if base.output_directory.is_empty() {
            base.output_directory = default_config.output_directory.clone();
        }
        if base.log_level.is_empty() {
            base.log_level = default_config.log_level.clone();
        }
        if base.video_format.is_empty() {
            base.video_format = default_config.video_format.clone();
        }
        if base.video_codec.is_empty() {
            base.video_codec = default_config.video_codec.clone();
        }
        if base.audio_codec.is_empty() {
            base.audio_codec = default_config.audio_codec.clone();
        }

        // 对于数值，只有当基础配置为0时才使用默认值
        if base.max_concurrent_downloads == 0 {
            base.max_concurrent_downloads = default_config.max_concurrent_downloads;
        }
        if base.buffer_size == 0 {
            base.buffer_size = default_config.buffer_size;
        }
        if base.chunk_size == 0 {
            base.chunk_size = default_config.chunk_size;
        }
    }
}

#[async_trait]
impl ConfigManager for DefaultConfigManager {
    async fn load_config(&self) -> Result<Config> {
        self.load_from_sources(self.config_sources.clone()).await
    }

    async fn validate_config(&self, config: &Config) -> Result<()> {
        let result = self.validator.validate_config(config);

        if !result.is_valid {
            let report = self.validator.get_validation_report(config);
            return Err(anyhow::anyhow!("配置验证失败:\n{}", report));
        }

        // 如果有警告或信息，记录到日志
        if result.has_warnings() || !result.infos.is_empty() {
            let report = self.validator.get_validation_report(config);
            log::warn!("配置验证通过但有建议:\n{}", report);
        }

        Ok(())
    }

    async fn reload_config(&self) -> Result<Config> {
        let config = self.load_config().await?;
        self.notify_config_change(config.clone()).await?;
        Ok(config)
    }

    fn watch_config_changes(&self) -> watch::Receiver<Config> {
        self.config_receiver.clone()
    }

    async fn load_from_sources(&self, sources: Vec<ConfigSource>) -> Result<Config> {
        let mut configs = Vec::new();

        for source in sources {
            match source {
                ConfigSource::Default => {
                    configs.push(Config::default());
                }
                ConfigSource::File(path) => match self.load_from_file(&path).await {
                    Ok(config) => configs.push(config),
                    Err(e) => {
                        log::warn!("无法从文件加载配置 {}: {}", path, e);
                    }
                },
                ConfigSource::Environment => match self.load_from_environment().await {
                    Ok(config) => configs.push(config),
                    Err(e) => {
                        log::warn!("无法从环境变量加载配置: {}", e);
                    }
                },
                ConfigSource::CommandLine(args) => match self.load_from_command_line(&args).await {
                    Ok(config) => configs.push(config),
                    Err(e) => {
                        log::warn!("无法从命令行参数加载配置: {}", e);
                    }
                },
            }
        }

        if configs.is_empty() {
            return Ok(Config::default());
        }

        self.merge_configs(configs).await
    }

    async fn merge_configs(&self, configs: Vec<Config>) -> Result<Config> {
        if configs.is_empty() {
            return Ok(Config::default());
        }

        let mut merged = configs[0].clone();

        for config in configs.iter().skip(1) {
            // 简单的覆盖策略：后面的配置直接覆盖前面的配置
            // 对于字符串，只有非空时才覆盖
            // 对于数值，直接覆盖
            // 对于布尔值，直接覆盖

            if !config.m3u8_urls.is_empty() {
                merged.m3u8_urls = config.m3u8_urls.clone();
            }

            merged.max_concurrent_downloads = config.max_concurrent_downloads;
            merged.download_retries = config.download_retries;
            merged.segment_timeout = config.segment_timeout;
            merged.retry_interval = config.retry_interval;

            if !config.output_directory.is_empty() {
                merged.output_directory = config.output_directory.clone();
            }

            if !config.log_level.is_empty() {
                merged.log_level = config.log_level.clone();
            }

            if !config.video_format.is_empty() {
                merged.video_format = config.video_format.clone();
            }

            if !config.video_codec.is_empty() {
                merged.video_codec = config.video_codec.clone();
            }

            if !config.audio_codec.is_empty() {
                merged.audio_codec = config.audio_codec.clone();
            }

            merged.buffer_size = config.buffer_size;
            merged.validate_segments = config.validate_segments;
            merged.use_hw_accel = config.use_hw_accel;
            merged.thread_count = config.thread_count;
            merged.chunk_size = config.chunk_size;

            if config.video_bitrate.is_some() {
                merged.video_bitrate = config.video_bitrate.clone();
            }

            if config.audio_bitrate.is_some() {
                merged.audio_bitrate = config.audio_bitrate.clone();
            }
        }

        Ok(merged)
    }

    async fn get_config_template(&self, template_name: &str) -> Result<Config> {
        self.config_templates
            .get(template_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("配置模板不存在: {}", template_name))
    }

    async fn resolve_config_conflicts(&self, configs: Vec<Config>) -> Result<Config> {
        // 使用优先级规则解决冲突：
        // 1. 命令行参数（最高优先级）
        // 2. 环境变量
        // 3. 配置文件
        // 4. 默认值（最低优先级）

        self.merge_configs(configs).await
    }

    async fn start_hot_reload(&self, watch_sources: Vec<ConfigWatchSource>) -> Result<()> {
        self.start_advanced_config_watching(watch_sources).await
    }

    async fn resolve_config_conflicts_with_priority(
        &self,
        configs: Vec<(Config, ConfigPriority)>,
    ) -> Result<Config> {
        self.resolve_config_conflicts_advanced(configs).await
    }
}

/// 默认视频处理服务实现
pub struct DefaultVideoProcessorService {
    /// M3U8解析器
    m3u8_parser: Arc<dyn crate::domain::services::M3U8Parser>,
    /// 下载管理器
    download_manager: Arc<dyn crate::domain::services::DownloadManager>,
    /// 片段处理器
    segment_processor: Arc<dyn crate::domain::services::SegmentProcessor>,
    /// FFmpeg处理器
    ffmpeg_handler: Arc<dyn crate::domain::services::FFmpegHandler>,
    /// 进度监控器
    progress_monitor: Arc<dyn crate::domain::services::ProgressMonitor>,
    /// 配置管理器
    config_manager: Arc<dyn ConfigManager>,
    /// 活跃任务列表
    active_tasks: Arc<RwLock<HashMap<String, TaskInfo>>>,
    /// 任务计数器
    task_counter: Arc<std::sync::atomic::AtomicU64>,
}

/// 任务信息
#[derive(Debug, Clone)]
struct TaskInfo {
    pub task_id: String,
    pub m3u8_url: String,
    pub config: Config,
    pub start_time: SystemTime,
    pub status: TaskStatus,
    pub progress: DownloadStats,
}

/// 任务状态
#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Initializing,
    Parsing,
    Downloading,
    Processing,
    Completed,
    Failed(String),
    Cancelled,
}

impl DefaultVideoProcessorService {
    pub fn new(
        m3u8_parser: Arc<dyn crate::domain::services::M3U8Parser>,
        download_manager: Arc<dyn crate::domain::services::DownloadManager>,
        segment_processor: Arc<dyn crate::domain::services::SegmentProcessor>,
        ffmpeg_handler: Arc<dyn crate::domain::services::FFmpegHandler>,
        progress_monitor: Arc<dyn crate::domain::services::ProgressMonitor>,
        config_manager: Arc<dyn ConfigManager>,
    ) -> Self {
        Self {
            m3u8_parser,
            download_manager,
            segment_processor,
            ffmpeg_handler,
            progress_monitor,
            config_manager,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            task_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// 生成新的任务ID
    fn generate_task_id(&self) -> String {
        let counter = self
            .task_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("task_{}_{}", timestamp, counter)
    }

    /// 更新任务状态
    async fn update_task_status(&self, task_id: &str, status: TaskStatus) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(task_info) = tasks.get_mut(task_id) {
            task_info.status = status;
            log::debug!("任务 {} 状态更新为: {:?}", task_id, task_info.status);
        }
        Ok(())
    }

    /// 更新任务进度
    async fn update_task_progress(&self, task_id: &str, progress: DownloadStats) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(task_info) = tasks.get_mut(task_id) {
            task_info.progress = progress;
        }
        Ok(())
    }

    /// 检查任务是否被取消
    async fn is_task_cancelled(&self, task_id: &str) -> bool {
        let tasks = self.active_tasks.read().await;
        if let Some(task_info) = tasks.get(task_id) {
            task_info.status == TaskStatus::Cancelled
        } else {
            false
        }
    }

    /// 处理视频流的核心逻辑
    async fn process_video_stream_internal(
        &self,
        task_id: &str,
        m3u8_url: &str,
        config: &Config,
    ) -> Result<ProcessingResult> {
        log::info!("开始处理视频流: {} (任务ID: {})", m3u8_url, task_id);

        // 1. 解析M3U8播放列表
        self.update_task_status(task_id, TaskStatus::Parsing)
            .await?;

        if self.is_task_cancelled(task_id).await {
            return Err(anyhow::anyhow!("任务已取消"));
        }

        let playlist = self
            .m3u8_parser
            .parse(m3u8_url)
            .await
            .with_context(|| format!("解析M3U8播放列表失败: {}", m3u8_url))?;

        log::info!("解析完成，找到 {} 个片段", playlist.segments.len());

        // 验证播放列表
        self.m3u8_parser.validate_playlist(&playlist)?;

        // 2. 开始进度监控
        self.progress_monitor.start_monitoring(task_id).await?;

        // 3. 下载片段
        self.update_task_status(task_id, TaskStatus::Downloading)
            .await?;

        if self.is_task_cancelled(task_id).await {
            return Err(anyhow::anyhow!("任务已取消"));
        }

        let segment_data = self
            .download_manager
            .download_segments(playlist.segments.clone())
            .await
            .with_context(|| "下载片段失败")?;

        log::info!("下载完成，成功下载 {} 个片段", segment_data.len());

        // 4. 处理片段（解密等）
        self.update_task_status(task_id, TaskStatus::Processing)
            .await?;

        if self.is_task_cancelled(task_id).await {
            return Err(anyhow::anyhow!("任务已取消"));
        }

        let processed_segments = self
            .segment_processor
            .process_segments(segment_data)
            .await
            .with_context(|| "处理片段失败")?;

        log::info!("片段处理完成，处理了 {} 个片段", processed_segments.len());

        // 5. 启动FFmpeg进程进行合并和编码
        let processing_config = self.create_processing_config(config).await?;
        let ffmpeg_process = self
            .ffmpeg_handler
            .start_process(&processing_config)
            .await
            .with_context(|| "启动FFmpeg进程失败")?;

        log::info!("FFmpeg进程启动成功，进程ID: {}", ffmpeg_process.process_id);

        // 6. 将处理后的片段数据写入FFmpeg进程
        for segment in &processed_segments {
            if self.is_task_cancelled(task_id).await {
                let _ = self.ffmpeg_handler.terminate_process(&ffmpeg_process).await;
                return Err(anyhow::anyhow!("任务已取消"));
            }

            ffmpeg_process
                .input_pipe
                .write_all(&segment.data)
                .await
                .with_context(|| format!("写入片段数据失败: {}", segment.index))?;

            // 更新进度
            let current_progress = self
                .progress_monitor
                .get_progress(task_id)
                .await
                .unwrap_or_default();
            self.update_task_progress(task_id, current_progress).await?;
        }

        // 7. 关闭输入管道并等待FFmpeg完成
        ffmpeg_process
            .input_pipe
            .flush()
            .await
            .with_context(|| "刷新FFmpeg输入管道失败")?;

        // 监控FFmpeg进程直到完成
        loop {
            if self.is_task_cancelled(task_id).await {
                let _ = self.ffmpeg_handler.terminate_process(&ffmpeg_process).await;
                return Err(anyhow::anyhow!("任务已取消"));
            }

            let status = self.ffmpeg_handler.monitor_process(&ffmpeg_process).await?;
            match status {
                crate::domain::services::ProcessStatus::Completed => {
                    log::info!("FFmpeg处理完成");
                    break;
                }
                crate::domain::services::ProcessStatus::Failed(error) => {
                    return Err(anyhow::anyhow!("FFmpeg处理失败: {}", error));
                }
                crate::domain::services::ProcessStatus::Running => {
                    // 继续等待
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                crate::domain::services::ProcessStatus::Terminated => {
                    return Err(anyhow::anyhow!("FFmpeg进程意外终止"));
                }
            }
        }

        // 8. 完成处理
        self.update_task_status(task_id, TaskStatus::Completed)
            .await?;
        self.progress_monitor.stop_monitoring(task_id).await?;

        let result = ProcessingResult {
            task_id: task_id.to_string(),
            output_file: ffmpeg_process.output_path.to_string_lossy().to_string(),
            success: true,
            error: None,
            stats: DownloadStats {
                total_segments: playlist.segments.len(),
                completed_segments: processed_segments.len(),
                failed_segments: 0,
                total_bytes: processed_segments.iter().map(|s| s.data.len() as u64).sum(),
                downloaded_bytes: processed_segments.iter().map(|s| s.data.len() as u64).sum(),
                download_speed: 0.0,
                estimated_time_remaining: None,
                start_time: ffmpeg_process.start_time,
                last_update: SystemTime::now(),
            },
            processing_time: SystemTime::now()
                .duration_since(ffmpeg_process.start_time)
                .unwrap_or_default(),
            created_at: SystemTime::now(),
        };

        log::info!("视频处理完成: {:?}", result);
        Ok(result)
    }

    /// 创建FFmpeg处理配置
    async fn create_processing_config(
        &self,
        config: &Config,
    ) -> Result<crate::domain::services::ProcessingConfig> {
        // 检测可用的硬件加速
        let hw_accelerators = self.ffmpeg_handler.detect_hardware_acceleration().await;
        let hardware_accel = if config.use_hw_accel && !hw_accelerators.is_empty() {
            hw_accelerators
                .into_iter()
                .find(|accel| *accel != crate::domain::services::HardwareAccelerator::None)
        } else {
            Some(crate::domain::services::HardwareAccelerator::None)
        };

        let mut custom_params = HashMap::new();

        // 添加自定义参数
        if let Some(ref bitrate) = config.video_bitrate {
            custom_params.insert("-maxrate".to_string(), bitrate.clone());
            custom_params.insert(
                "-bufsize".to_string(),
                format!(
                    "{}k",
                    bitrate.trim_end_matches('k').parse::<u32>().unwrap_or(2000) * 2
                ),
            );
        }

        if let Some(ref _bitrate) = config.audio_bitrate {
            custom_params.insert("-ar".to_string(), "44100".to_string()); // 音频采样率
        }

        Ok(crate::domain::services::ProcessingConfig {
            video_codec: config.video_codec.clone(),
            audio_codec: config.audio_codec.clone(),
            video_bitrate: config.video_bitrate.clone(),
            audio_bitrate: config.audio_bitrate.clone(),
            hardware_accel,
            thread_count: config.thread_count,
            output_format: config.video_format.clone(),
            custom_params,
        })
    }

    /// 获取文件大小
    async fn get_file_size(&self, path: &std::path::Path) -> Result<u64> {
        let metadata = tokio::fs::metadata(path).await?;
        Ok(metadata.len())
    }
    async fn cleanup_directory(&self, dir_path: &str, pattern: &str) -> Result<u64> {
        let mut cleaned_size = 0u64;
        let mut entries = tokio::fs::read_dir(dir_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.contains(pattern)
                    || file_name.starts_with("tmp_")
                    || file_name.ends_with(".tmp")
                {
                    if let Ok(metadata) = entry.metadata().await {
                        cleaned_size += metadata.len();
                    }

                    if let Err(e) = tokio::fs::remove_file(&path).await {
                        log::warn!("删除临时文件失败: {} - {}", path.display(), e);
                    } else {
                        log::debug!("删除临时文件: {}", path.display());
                    }
                }
            }
        }

        Ok(cleaned_size)
    }
}

#[async_trait]
impl VideoProcessorService for DefaultVideoProcessorService {
    async fn process_video_stream(
        &self,
        m3u8_url: &str,
        config: &Config,
    ) -> Result<ProcessingResult> {
        let task_id = self.generate_task_id();

        // 创建任务信息
        let task_info = TaskInfo {
            task_id: task_id.clone(),
            m3u8_url: m3u8_url.to_string(),
            config: config.clone(),
            start_time: SystemTime::now(),
            status: TaskStatus::Initializing,
            progress: DownloadStats::default(),
        };

        // 添加到活跃任务列表
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), task_info);
        }

        // 执行处理
        let result = self
            .process_video_stream_internal(&task_id, m3u8_url, config)
            .await;

        // 处理结果
        match &result {
            Ok(_) => {
                log::info!("任务 {} 成功完成", task_id);
            }
            Err(e) => {
                log::error!("任务 {} 失败: {}", task_id, e);
                let _ = self
                    .update_task_status(&task_id, TaskStatus::Failed(e.to_string()))
                    .await;
            }
        }

        // 从活跃任务列表中移除（可选，也可以保留用于历史查询）
        // self.active_tasks.write().await.remove(&task_id);

        result
    }

    async fn get_processing_progress(&self, task_id: &str) -> Result<DownloadStats> {
        let tasks = self.active_tasks.read().await;
        if let Some(task_info) = tasks.get(task_id) {
            Ok(task_info.progress.clone())
        } else {
            // 尝试从进度监控器获取
            self.progress_monitor
                .get_progress(task_id)
                .await
                .or_else(|_| Ok(DownloadStats::default()))
        }
    }

    async fn cancel_processing(&self, task_id: &str) -> Result<()> {
        log::info!("取消任务: {}", task_id);

        // 更新任务状态为已取消
        self.update_task_status(task_id, TaskStatus::Cancelled)
            .await?;

        // 停止进度监控
        let _ = self.progress_monitor.stop_monitoring(task_id).await;

        // 取消下载管理器中的下载
        let _ = self.download_manager.cancel_downloads().await;

        log::info!("任务 {} 已取消", task_id);
        Ok(())
    }

    async fn cleanup_temp_files(&self, config: &Config) -> Result<()> {
        log::info!("开始清理临时文件");

        let mut total_cleaned = 0u64;

        // 清理输出目录中的临时文件
        if let Ok(cleaned) = self
            .cleanup_directory(&config.output_directory, "tmp")
            .await
        {
            total_cleaned += cleaned;
        }

        // 清理系统临时目录
        if let Ok(temp_dir) = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TMP"))
            .or_else(|_| std::env::var("TEMP"))
        {
            if let Ok(cleaned) = self.cleanup_directory(&temp_dir, "video_processor").await {
                total_cleaned += cleaned;
            }
        } else {
            // 默认临时目录
            let default_temp_dirs = ["/tmp", "/var/tmp", "C:\\Windows\\Temp"];
            for temp_dir in &default_temp_dirs {
                if let Ok(cleaned) = self.cleanup_directory(temp_dir, "video_processor").await {
                    total_cleaned += cleaned;
                }
            }
        }

        // 清理当前工作目录中的临时文件
        if let Ok(current_dir) = std::env::current_dir() {
            if let Some(current_dir_str) = current_dir.to_str() {
                if let Ok(cleaned) = self.cleanup_directory(current_dir_str, "tmp").await {
                    total_cleaned += cleaned;
                }
            }
        }

        log::info!("临时文件清理完成，释放空间: {} 字节", total_cleaned);
        Ok(())
    }
}
