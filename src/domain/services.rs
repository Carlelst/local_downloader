/// 领域服务接口定义
/// 定义核心业务逻辑的抽象接口
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::entities::*;

/// M3U8解析器领域服务
#[async_trait]
pub trait M3U8Parser: Send + Sync {
    /// 解析M3U8播放列表
    async fn parse(&self, url: &str) -> Result<Playlist>;

    /// 解析片段列表
    async fn resolve_segments(&self, playlist: &Playlist) -> Result<Vec<Segment>>;

    /// 验证播放列表格式
    fn validate_playlist(&self, playlist: &Playlist) -> Result<()>;

    /// 检测播放列表类型
    fn detect_playlist_type(&self, content: &str) -> Result<PlaylistType>;
}

/// 播放列表类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistType {
    Master,  // 主播放列表
    Media,   // 媒体播放列表
    Unknown, // 未知类型
}

/// 下载管理器领域服务
#[async_trait]
pub trait DownloadManager: Send + Sync {
    /// 下载片段列表
    async fn download_segments(&self, segments: Vec<Segment>) -> Result<HashMap<usize, Vec<u8>>>;

    /// 设置并发级别
    async fn set_concurrency(&self, level: usize) -> Result<()>;

    /// 获取下载统计
    fn get_download_stats(&self) -> DownloadStats;

    /// 暂停下载
    async fn pause_downloads(&self) -> Result<()>;

    /// 恢复下载
    async fn resume_downloads(&self) -> Result<()>;

    /// 取消所有下载
    async fn cancel_downloads(&self) -> Result<()>;
}

/// 片段处理器领域服务
#[async_trait]
pub trait SegmentProcessor: Send + Sync {
    /// 处理单个片段
    async fn process_segment(&self, segment: &Segment, data: &[u8]) -> Result<ProcessedSegment>;

    /// 批量处理片段
    async fn process_segments(
        &self,
        segments: HashMap<usize, Vec<u8>>,
    ) -> Result<Vec<ProcessedSegment>>;

    /// 解密片段（如果需要）
    async fn decrypt_if_needed(&self, data: &[u8], encryption: &EncryptionInfo) -> Result<Vec<u8>>;

    /// 验证片段完整性
    fn validate_segment(&self, segment: &ProcessedSegment) -> Result<()>;
}

/// 处理后的片段
#[derive(Debug, Clone)]
pub struct ProcessedSegment {
    pub index: usize,
    pub data: Vec<u8>,
    pub metadata: SegmentMetadata,
    pub processing_time: std::time::Duration,
    pub is_encrypted: bool,
    pub is_valid: bool,
}

/// FFmpeg处理器领域服务
#[async_trait]
pub trait FFmpegHandler: Send + Sync {
    /// 启动FFmpeg进程
    async fn start_process(&self, config: &ProcessingConfig) -> Result<FFmpegProcess>;

    /// 检测硬件加速支持
    async fn detect_hardware_acceleration(&self) -> Vec<HardwareAccelerator>;

    /// 优化编码参数
    async fn optimize_encoding_params(&self, input_info: &InputInfo) -> EncodingParams;

    /// 监控进程状态
    async fn monitor_process(&self, process: &FFmpegProcess) -> Result<ProcessStatus>;

    /// 终止进程
    async fn terminate_process(&self, process: &FFmpegProcess) -> Result<()>;
}

/// 同步安全的输入管道包装器
pub struct SyncInputPipe {
    inner: Arc<Mutex<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>>,
}

impl SyncInputPipe {
    pub fn new(pipe: Box<dyn tokio::io::AsyncWrite + Send + Unpin>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(pipe)),
        }
    }

    pub async fn write_all(&self, buf: &[u8]) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut pipe = self.inner.lock().await;
        pipe.write_all(buf).await
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut pipe = self.inner.lock().await;
        pipe.flush().await
    }
}

unsafe impl Send for SyncInputPipe {}
unsafe impl Sync for SyncInputPipe {}

/// FFmpeg进程信息
pub struct FFmpegProcess {
    pub process_id: u32,
    pub input_pipe: SyncInputPipe,
    pub output_path: std::path::PathBuf,
    pub config: ProcessingConfig,
    pub start_time: std::time::SystemTime,
}

impl std::fmt::Debug for FFmpegProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FFmpegProcess")
            .field("process_id", &self.process_id)
            .field("output_path", &self.output_path)
            .field("config", &self.config)
            .field("start_time", &self.start_time)
            .finish()
    }
}

/// 处理配置
#[derive(Debug, Clone)]
pub struct ProcessingConfig {
    pub video_codec: String,
    pub audio_codec: String,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub hardware_accel: Option<HardwareAccelerator>,
    pub thread_count: i32,
    pub output_format: String,
    pub custom_params: HashMap<String, String>,
}

/// 硬件加速器类型
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareAccelerator {
    VideoToolbox, // macOS
    Nvenc,        // NVIDIA
    Qsv,          // Intel QuickSync
    Cuda,         // NVIDIA CUDA
    Vaapi,        // Linux VA-API
    None,         // 软件编码
}

/// 输入信息
#[derive(Debug, Clone)]
pub struct InputInfo {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    pub bitrate: Option<u64>,
    pub duration: Option<std::time::Duration>,
}

/// 编码参数
#[derive(Debug, Clone)]
pub struct EncodingParams {
    pub video_codec: String,
    pub audio_codec: String,
    pub video_bitrate: String,
    pub audio_bitrate: String,
    pub preset: String,
    pub crf: Option<u8>,
    pub additional_params: Vec<String>,
}

/// 进程状态
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Running,
    Completed,
    Failed(String),
    Terminated,
}

/// 配置管理器领域服务
#[async_trait]
pub trait ConfigManager: Send + Sync {
    /// 加载配置
    async fn load_config(&self, sources: Vec<ConfigSource>) -> Result<Config>;

    /// 验证配置
    async fn validate_config(&self, config: &Config) -> Result<()>;

    /// 重新加载配置
    async fn reload_config(&self) -> Result<Config>;

    /// 监听配置变化
    async fn watch_config_changes(&self) -> Result<tokio::sync::mpsc::Receiver<Config>>;

    /// 合并配置
    fn merge_configs(&self, configs: Vec<Config>) -> Result<Config>;
}

/// 配置源类型
#[derive(Debug, Clone)]
pub enum ConfigSource {
    File(std::path::PathBuf),
    Environment,
    CommandLine(HashMap<String, String>),
    Default,
}

/// 进度监控器领域服务
#[async_trait]
pub trait ProgressMonitor: Send + Sync {
    /// 开始监控任务
    async fn start_monitoring(&self, task_id: &str) -> Result<()>;

    /// 更新进度
    async fn update_progress(&self, task_id: &str, stats: &DownloadStats) -> Result<()>;

    /// 获取当前进度
    async fn get_progress(&self, task_id: &str) -> Result<DownloadStats>;

    /// 停止监控
    async fn stop_monitoring(&self, task_id: &str) -> Result<()>;

    /// 获取所有活跃任务
    async fn get_active_tasks(&self) -> Result<Vec<String>>;
}

/// 错误处理器领域服务
pub trait ErrorHandler: Send + Sync {
    /// 分类错误
    fn classify_error(&self, error: &anyhow::Error) -> ErrorCategory;

    /// 确定恢复策略
    fn determine_recovery_strategy(&self, error: &anyhow::Error) -> RecoveryStrategy;

    /// 执行恢复操作
    fn execute_recovery(&self, strategy: &RecoveryStrategy) -> Result<()>;

    /// 记录错误
    fn log_error(&self, error: &anyhow::Error, context: &str) -> Result<()>;
}

/// 错误分类
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    Network,       // 网络错误
    Parsing,       // 解析错误
    Processing,    // 处理错误
    Configuration, // 配置错误
    System,        // 系统错误
    Unknown,       // 未知错误
}

/// 恢复策略
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryStrategy {
    Retry {
        max_attempts: u32,
        delay: std::time::Duration,
    },
    Skip,
    Fallback {
        alternative: String,
    },
    Abort,
    UserIntervention,
}

/// 字幕处理器领域服务
#[async_trait]
pub trait SubtitleProcessor: Send + Sync {
    /// 下载字幕文件
    async fn download_subtitle(&self, url: &str) -> Result<SubtitleFile>;

    /// 解析字幕文件
    async fn parse_subtitle(&self, content: &[u8], format: SubtitleFormat) -> Result<Subtitle>;

    /// 转换字幕格式
    async fn convert_subtitle(
        &self,
        subtitle: &Subtitle,
        target_format: SubtitleFormat,
    ) -> Result<Vec<u8>>;

    /// 同步字幕时间轴
    async fn sync_subtitle(
        &self,
        subtitle: &mut Subtitle,
        offset: std::time::Duration,
    ) -> Result<()>;

    /// 验证字幕完整性
    fn validate_subtitle(&self, subtitle: &Subtitle) -> Result<()>;

    /// 嵌入字幕到视频
    async fn embed_subtitle(
        &self,
        video_path: &str,
        subtitle: &Subtitle,
        output_path: &str,
    ) -> Result<()>;
}

/// 字幕文件
#[derive(Debug, Clone)]
pub struct SubtitleFile {
    pub url: String,
    pub content: Vec<u8>,
    pub format: SubtitleFormat,
    pub language: Option<String>,
    pub encoding: Option<String>,
}

/// 字幕内容
#[derive(Debug, Clone)]
pub struct Subtitle {
    pub entries: Vec<SubtitleEntry>,
    pub format: SubtitleFormat,
    pub language: Option<String>,
    pub metadata: SubtitleMetadata,
}

/// 字幕条目
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    pub index: usize,
    pub start_time: std::time::Duration,
    pub end_time: std::time::Duration,
    pub text: String,
    pub style: Option<SubtitleStyle>,
}

/// 字幕格式
#[derive(Debug, Clone, PartialEq)]
pub enum SubtitleFormat {
    Srt,  // SubRip
    Vtt,  // WebVTT
    Ass,  // Advanced SubStation Alpha
    Ssa,  // SubStation Alpha
    Ttml, // Timed Text Markup Language
    Scc,  // Scenarist Closed Caption
    Unknown,
}

/// 字幕样式
#[derive(Debug, Clone)]
pub struct SubtitleStyle {
    pub font_name: Option<String>,
    pub font_size: Option<u32>,
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// 字幕元数据
#[derive(Debug, Clone, Default)]
pub struct SubtitleMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub creation_date: Option<std::time::SystemTime>,
    pub total_entries: usize,
    pub duration: Option<std::time::Duration>,
}
