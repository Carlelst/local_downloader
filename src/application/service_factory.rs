/// 服务工厂 - 负责创建和配置所有应用服务
///
/// 该模块提供依赖注入和服务配置功能，用于集成新架构与现有代码
use anyhow::Result;
use std::sync::Arc;

use crate::application::services::{
    ConfigManager, DefaultConfigManager, DefaultVideoProcessorService, VideoProcessorService,
};
use crate::domain::entities::Config;
use crate::domain::services::{
    DownloadManager, FFmpegHandler, M3U8Parser, ProgressMonitor, SegmentProcessor,
};
use crate::infrastructure::{
    AdaptiveDownloadManager, DefaultFFmpegHandler, DefaultHttpClient, DefaultM3U8Parser,
    DefaultProgressMonitor, DefaultSegmentProcessor, DownloadConfig, JsonFilePersistence,
};

/// 服务工厂 - 创建和配置所有服务实例
pub struct ServiceFactory {
    config: Config,
}

impl ServiceFactory {
    /// 创建新的服务工厂实例
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// 创建完整的视频处理服务
    pub async fn create_video_processor_service(&self) -> Result<Arc<dyn VideoProcessorService>> {
        // 创建基础设施层服务
        let http_client = Arc::new(DefaultHttpClient::new(self.config.segment_timeout)?);
        let filesystem = Box::new(crate::infrastructure::DefaultFileSystem::new());
        let persistence = Arc::new(JsonFilePersistence::new(
            std::path::PathBuf::from("./temp"),
            filesystem,
        ));

        // 创建M3U8解析器
        let m3u8_parser: Arc<dyn M3U8Parser> = Arc::new(DefaultM3U8Parser::new(Box::new(
            DefaultHttpClient::new(self.config.segment_timeout)?,
        )));

        // 创建下载管理器
        let download_config = DownloadConfig {
            max_concurrent_downloads: self.config.max_concurrent_downloads,
            min_concurrent_downloads: 1,
            chunk_size: self.config.chunk_size,
            timeout_secs: self.config.segment_timeout,
            max_retries: self.config.download_retries as u32,
            enable_resume: true,
            bandwidth_threshold_mbps: 10.0,
            latency_threshold_ms: 1000,
        };

        let download_manager: Arc<dyn DownloadManager> = Arc::new(AdaptiveDownloadManager::new(
            http_client.clone(),
            persistence.clone(),
            download_config,
        ));

        // 创建片段处理器
        let segment_processor: Arc<dyn SegmentProcessor> = Arc::new(DefaultSegmentProcessor::new());

        // 创建FFmpeg处理器
        let ffmpeg_handler: Arc<dyn FFmpegHandler> = Arc::new(DefaultFFmpegHandler::new());

        // 创建进度监控器
        let progress_monitor: Arc<dyn ProgressMonitor> =
            Arc::new(DefaultProgressMonitor::with_defaults());

        // 创建配置管理器
        let config_manager: Arc<dyn ConfigManager> = Arc::new(DefaultConfigManager::new());

        // 创建视频处理服务
        let video_processor_service = Arc::new(DefaultVideoProcessorService::new(
            m3u8_parser,
            download_manager,
            segment_processor,
            ffmpeg_handler,
            progress_monitor,
            config_manager,
        ));

        Ok(video_processor_service)
    }

    /// 创建配置管理器
    pub fn create_config_manager(&self) -> Arc<dyn ConfigManager> {
        Arc::new(DefaultConfigManager::new())
    }

    /// 创建M3U8解析器
    pub fn create_m3u8_parser(&self) -> Result<Arc<dyn M3U8Parser>> {
        Ok(Arc::new(DefaultM3U8Parser::new(Box::new(
            DefaultHttpClient::new(self.config.segment_timeout)?,
        ))))
    }

    /// 创建下载管理器
    pub fn create_download_manager(&self) -> Result<Arc<dyn DownloadManager>> {
        let http_client = Arc::new(DefaultHttpClient::new(self.config.segment_timeout)?);
        let filesystem = Box::new(crate::infrastructure::DefaultFileSystem::new());
        let persistence = Arc::new(JsonFilePersistence::new(
            std::path::PathBuf::from("./temp"),
            filesystem,
        ));

        let download_config = DownloadConfig {
            max_concurrent_downloads: self.config.max_concurrent_downloads,
            min_concurrent_downloads: 1,
            chunk_size: self.config.chunk_size,
            timeout_secs: self.config.segment_timeout,
            max_retries: self.config.download_retries as u32,
            enable_resume: true,
            bandwidth_threshold_mbps: 10.0,
            latency_threshold_ms: 1000,
        };

        Ok(Arc::new(AdaptiveDownloadManager::new(
            http_client,
            persistence,
            download_config,
        )))
    }

    /// 创建FFmpeg处理器
    pub fn create_ffmpeg_handler(&self) -> Arc<dyn FFmpegHandler> {
        Arc::new(DefaultFFmpegHandler::new())
    }

    /// 创建进度监控器
    pub fn create_progress_monitor(&self) -> Arc<dyn ProgressMonitor> {
        Arc::new(DefaultProgressMonitor::with_defaults())
    }
}

/// 向后兼容的服务适配器
///
/// 该结构体提供了一个桥梁，允许现有代码逐步迁移到新架构
pub struct LegacyServiceAdapter {
    video_processor_service: Arc<dyn VideoProcessorService>,
    config: Config,
}

impl LegacyServiceAdapter {
    /// 创建新的适配器实例
    pub async fn new(config: Config) -> Result<Self> {
        let factory = ServiceFactory::new(config.clone());
        let video_processor_service = factory.create_video_processor_service().await?;

        Ok(Self {
            video_processor_service,
            config,
        })
    }

    /// 处理单个M3U8 URL（向后兼容接口）
    pub async fn process_m3u8_url(
        &self,
        m3u8_url: &str,
    ) -> Result<crate::domain::entities::ProcessingResult> {
        // 将旧的Config转换为新的domain Config
        let domain_config = self.convert_to_domain_config();

        self.video_processor_service
            .process_video_stream(m3u8_url, &domain_config)
            .await
    }

    /// 获取处理进度
    pub async fn get_progress(
        &self,
        task_id: &str,
    ) -> Result<crate::domain::entities::DownloadStats> {
        self.video_processor_service
            .get_processing_progress(task_id)
            .await
    }

    /// 取消处理
    pub async fn cancel_processing(&self, task_id: &str) -> Result<()> {
        self.video_processor_service
            .cancel_processing(task_id)
            .await
    }

    /// 清理临时文件
    pub async fn cleanup_temp_files(&self) -> Result<()> {
        let domain_config = self.convert_to_domain_config();
        self.video_processor_service
            .cleanup_temp_files(&domain_config)
            .await
    }

    /// 将旧的Config转换为新的domain Config
    fn convert_to_domain_config(&self) -> crate::domain::entities::Config {
        crate::domain::entities::Config {
            m3u8_urls: self.config.m3u8_urls.clone(),
            max_concurrent_downloads: self.config.max_concurrent_downloads,
            download_retries: self.config.download_retries,
            segment_timeout: self.config.segment_timeout,
            retry_interval: self.config.retry_interval,
            output_directory: self.config.output_directory.clone(),
            log_level: self.config.log_level.clone(),
            video_format: self.config.video_format.clone(),
            video_codec: self.config.video_codec.clone(),
            audio_codec: self.config.audio_codec.clone(),
            buffer_size: self.config.buffer_size,
            validate_segments: self.config.validate_segments,
            use_hw_accel: self.config.use_hw_accel,
            thread_count: self.config.thread_count,
            video_bitrate: self.config.video_bitrate.clone(),
            audio_bitrate: self.config.audio_bitrate.clone(),
            chunk_size: self.config.chunk_size,
        }
    }
}

/// 创建默认的服务工厂
pub fn create_default_service_factory(config: Config) -> ServiceFactory {
    ServiceFactory::new(config)
}

/// 创建向后兼容的服务适配器
pub async fn create_legacy_adapter(config: Config) -> Result<LegacyServiceAdapter> {
    LegacyServiceAdapter::new(config).await
}
