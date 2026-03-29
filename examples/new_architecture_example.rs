/// 新架构集成示例
///
/// 该示例展示了如何使用新的分层架构来处理视频流，
/// 同时保持与现有代码的兼容性
use anyhow::Result;
use log::info;
use std::env;
use video_processor::{
    application::{create_legacy_adapter, ServiceFactory},
    domain::entities::Config as DomainConfig,
    Config,
};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    env_logger::init();

    // 加载配置（使用现有的配置加载机制）
    let config = match video_processor::load_config() {
        Ok(config) => config,
        Err(_) => {
            // 如果配置文件不存在，使用默认配置
            info!("使用默认配置");
            create_default_config()
        }
    };

    info!("配置加载完成: {:?}", config);

    // 演示方式1：使用向后兼容的适配器
    info!("=== 演示方式1：使用向后兼容适配器 ===");
    demonstrate_legacy_adapter(config.clone()).await?;

    // 演示方式2：直接使用新架构的服务工厂
    info!("=== 演示方式2：使用新架构服务工厂 ===");
    demonstrate_service_factory(config.clone()).await?;

    Ok(())
}

/// 演示使用向后兼容适配器
async fn demonstrate_legacy_adapter(config: Config) -> Result<()> {
    // 创建向后兼容适配器
    let adapter = create_legacy_adapter(config).await?;

    // 使用适配器处理视频流（如果配置中有URL）
    if let Some(m3u8_url) = get_test_url() {
        info!("使用适配器处理M3U8 URL: {}", m3u8_url);

        match adapter.process_m3u8_url(&m3u8_url).await {
            Ok(result) => {
                info!("处理成功: {:?}", result);
            }
            Err(e) => {
                info!("处理失败（这是正常的，因为这只是演示）: {}", e);
            }
        }
    } else {
        info!("没有提供测试URL，跳过实际处理");
    }

    // 演示清理功能
    info!("执行临时文件清理...");
    adapter.cleanup_temp_files().await?;

    Ok(())
}

/// 演示使用服务工厂
async fn demonstrate_service_factory(config: Config) -> Result<()> {
    // 创建服务工厂
    let factory = ServiceFactory::new(convert_to_domain_config(&config));

    // 创建各种服务
    let video_processor_service = factory.create_video_processor_service().await?;
    let config_manager = factory.create_config_manager();
    let m3u8_parser = factory.create_m3u8_parser()?;
    let download_manager = factory.create_download_manager()?;
    let ffmpeg_handler = factory.create_ffmpeg_handler();
    let progress_monitor = factory.create_progress_monitor();

    info!("所有服务创建成功");

    // 演示配置管理
    let domain_config = config_manager.load_config().await?;
    info!("加载的领域配置: {:?}", domain_config);

    // 演示硬件加速检测
    let hw_accelerators = ffmpeg_handler.detect_hardware_acceleration().await;
    info!("检测到的硬件加速器: {:?}", hw_accelerators);

    // 如果有测试URL，演示完整的处理流程
    if let Some(m3u8_url) = get_test_url() {
        info!("使用服务工厂处理M3U8 URL: {}", m3u8_url);

        match video_processor_service
            .process_video_stream(&m3u8_url, &domain_config)
            .await
        {
            Ok(result) => {
                info!("处理成功: {:?}", result);
            }
            Err(e) => {
                info!("处理失败（这是正常的，因为这只是演示）: {}", e);
            }
        }
    } else {
        info!("没有提供测试URL，跳过实际处理");
    }

    Ok(())
}

/// 创建默认配置
fn create_default_config() -> Config {
    Config {
        m3u8_urls: vec![],
        max_concurrent_downloads: 4,
        download_retries: 3,
        segment_timeout: 30,
        retry_interval: 2,
        output_directory: "./output".to_string(),
        log_level: "info".to_string(),
        video_format: "mp4".to_string(),
        video_codec: "libx264".to_string(),
        audio_codec: "aac".to_string(),
        buffer_size: 4 * 1024 * 1024,
        validate_segments: false,
        use_hw_accel: true,
        thread_count: 0,
        video_bitrate: Some("2M".to_string()),
        audio_bitrate: Some("128k".to_string()),
        chunk_size: 1024 * 1024,
    }
}

/// 将旧配置转换为领域配置
fn convert_to_domain_config(config: &Config) -> DomainConfig {
    DomainConfig {
        m3u8_urls: config.m3u8_urls.clone(),
        max_concurrent_downloads: config.max_concurrent_downloads,
        download_retries: config.download_retries,
        segment_timeout: config.segment_timeout,
        retry_interval: config.retry_interval,
        output_directory: config.output_directory.clone(),
        log_level: config.log_level.clone(),
        video_format: config.video_format.clone(),
        video_codec: config.video_codec.clone(),
        audio_codec: config.audio_codec.clone(),
        buffer_size: config.buffer_size,
        validate_segments: config.validate_segments,
        use_hw_accel: config.use_hw_accel,
        thread_count: config.thread_count,
        video_bitrate: config.video_bitrate.clone(),
        audio_bitrate: config.audio_bitrate.clone(),
        chunk_size: config.chunk_size,
    }
}

/// 获取测试URL（从环境变量或命令行参数）
fn get_test_url() -> Option<String> {
    // 首先尝试从环境变量获取
    if let Ok(url) = env::var("TEST_M3U8_URL") {
        return Some(url);
    }

    // 然后尝试从命令行参数获取
    let args: Vec<String> = env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        if arg == "--url" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }

    None
}
