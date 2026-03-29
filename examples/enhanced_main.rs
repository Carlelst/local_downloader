/// 增强的主程序示例
///
/// 该示例展示了如何使用新架构重构主程序，
/// 同时保持与现有命令行参数的兼容性
use anyhow::{anyhow, Result};
use chrono::Local;
use env_logger::Builder;
use log::{debug, error, info, warn};
use std::env;
use std::io::Write;
use std::time::Instant;
use tokio::time::Duration;

use video_processor::{
    application::{create_legacy_adapter, ServiceFactory},
    domain::entities::Config as DomainConfig,
    load_config, parse_log_level, Config,
};

/// 命令行参数结构
#[derive(Debug, Clone)]
struct CliArgs {
    pub use_new_architecture: bool,
    pub clear_cache: bool,
    pub force_redownload: bool,
    pub video_bitrate: Option<String>,
    pub use_copy_mode: bool,
    pub disable_hw_accel: bool,
    pub config_file: Option<String>,
    pub urls: Vec<String>,
    pub output_directory: Option<String>,
    pub max_concurrent: Option<usize>,
    pub log_level: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();

    // 解析命令行参数
    let cli_args = parse_command_line_args();

    // 加载配置
    let mut config = load_configuration(&cli_args).await?;

    // 应用命令行参数覆盖
    apply_cli_overrides(&mut config, &cli_args);

    // 初始化日志
    init_logging(&config);

    info!("视频处理器启动");
    info!(
        "使用架构: {}",
        if cli_args.use_new_architecture {
            "新架构"
        } else {
            "传统架构"
        }
    );
    info!("配置: {:?}", config);

    // 处理特殊命令
    if cli_args.clear_cache {
        clear_cache(&config).await?;
    }

    // 选择处理方式
    let result = if cli_args.use_new_architecture {
        process_with_new_architecture(config, cli_args).await
    } else {
        process_with_legacy_architecture(config, cli_args).await
    };

    match result {
        Ok(_) => {
            info!("所有任务完成，总耗时: {:.2?}", start_time.elapsed());
        }
        Err(e) => {
            error!("处理失败: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// 解析命令行参数
fn parse_command_line_args() -> CliArgs {
    let args: Vec<String> = env::args().collect();
    let mut cli_args = CliArgs {
        use_new_architecture: false,
        clear_cache: false,
        force_redownload: false,
        video_bitrate: None,
        use_copy_mode: false,
        disable_hw_accel: false,
        config_file: None,
        urls: Vec::new(),
        output_directory: None,
        max_concurrent: None,
        log_level: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--new-arch" | "--use-new-architecture" => {
                cli_args.use_new_architecture = true;
            }
            "--clear-cache" => {
                cli_args.clear_cache = true;
            }
            "--force" => {
                cli_args.force_redownload = true;
            }
            "--copy" => {
                cli_args.use_copy_mode = true;
            }
            "--no-hw" => {
                cli_args.disable_hw_accel = true;
            }
            "--bitrate" => {
                if i + 1 < args.len() {
                    cli_args.video_bitrate = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--config" => {
                if i + 1 < args.len() {
                    cli_args.config_file = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--output" | "-o" => {
                if i + 1 < args.len() {
                    cli_args.output_directory = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--concurrent" | "-c" => {
                if i + 1 < args.len() {
                    if let Ok(concurrent) = args[i + 1].parse() {
                        cli_args.max_concurrent = Some(concurrent);
                    }
                    i += 1;
                }
            }
            "--log-level" => {
                if i + 1 < args.len() {
                    cli_args.log_level = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--url" => {
                if i + 1 < args.len() {
                    cli_args.urls.push(args[i + 1].clone());
                    i += 1;
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            arg if arg.starts_with("http") => {
                // 直接的URL参数
                cli_args.urls.push(arg.to_string());
            }
            _ => {
                // 忽略未知参数
            }
        }
        i += 1;
    }

    cli_args
}

/// 打印帮助信息
fn print_help() {
    println!("视频处理器 - M3U8流媒体下载和处理工具");
    println!();
    println!("用法:");
    println!("  video_processor [选项] [URL...]");
    println!();
    println!("选项:");
    println!("  --new-arch              使用新的分层架构");
    println!("  --clear-cache           清除下载缓存");
    println!("  --force                 强制重新下载所有片段");
    println!("  --copy                  使用copy模式，不重新编码");
    println!("  --no-hw                 禁用硬件加速");
    println!("  --bitrate <RATE>        设置视频比特率 (例: 2M)");
    println!("  --config <FILE>         指定配置文件路径");
    println!("  --output <DIR>          设置输出目录");
    println!("  --concurrent <NUM>      设置最大并发下载数");
    println!("  --log-level <LEVEL>     设置日志级别 (debug/info/warn/error)");
    println!("  --url <URL>             添加要处理的M3U8 URL");
    println!("  --help, -h              显示此帮助信息");
    println!();
    println!("示例:");
    println!("  video_processor --new-arch --output ./downloads https://example.com/playlist.m3u8");
    println!("  video_processor --copy --no-hw --bitrate 1M https://example.com/stream.m3u8");
}

/// 加载配置
async fn load_configuration(cli_args: &CliArgs) -> Result<Config> {
    if let Some(ref config_file) = cli_args.config_file {
        // 尝试从指定的配置文件加载
        match tokio::fs::read_to_string(config_file).await {
            Ok(content) => {
                let config: Config = serde_yaml::from_str(&content)
                    .map_err(|e| anyhow!("解析配置文件失败: {}", e))?;
                info!("从配置文件加载配置: {}", config_file);
                Ok(config)
            }
            Err(e) => {
                warn!("无法读取配置文件 {}: {}", config_file, e);
                Ok(create_default_config())
            }
        }
    } else {
        // 尝试加载默认配置文件
        match load_config() {
            Ok(config) => {
                info!("加载默认配置文件");
                Ok(config)
            }
            Err(_) => {
                info!("使用默认配置");
                Ok(create_default_config())
            }
        }
    }
}

/// 创建默认配置
fn create_default_config() -> Config {
    Config {
        m3u8_urls: Vec::new(),
        max_concurrent_downloads: 8,
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
        video_bitrate: None,
        audio_bitrate: None,
        chunk_size: 1024 * 1024,
    }
}

/// 应用命令行参数覆盖
fn apply_cli_overrides(config: &mut Config, cli_args: &CliArgs) {
    // 添加命令行指定的URL
    config.m3u8_urls.extend(cli_args.urls.clone());

    // 应用其他覆盖
    if let Some(ref output_dir) = cli_args.output_directory {
        config.output_directory = output_dir.clone();
    }

    if let Some(concurrent) = cli_args.max_concurrent {
        config.max_concurrent_downloads = concurrent;
    }

    if let Some(ref log_level) = cli_args.log_level {
        config.log_level = log_level.clone();
    }

    if let Some(ref bitrate) = cli_args.video_bitrate {
        config.video_bitrate = Some(bitrate.clone());
    }

    if cli_args.use_copy_mode {
        config.video_codec = "copy".to_string();
        config.use_hw_accel = false;
    }

    if cli_args.disable_hw_accel {
        config.use_hw_accel = false;
    }
}

/// 初始化日志
fn init_logging(config: &Config) {
    Builder::new()
        .filter(None, parse_log_level(&config.log_level))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
}

/// 清除缓存
async fn clear_cache(config: &Config) -> Result<()> {
    info!("清除所有下载缓存...");

    let cache_dirs = [
        format!("{}/temp", config.output_directory),
        "./temp".to_string(),
        "/tmp/video_processor".to_string(),
    ];

    for cache_dir in &cache_dirs {
        if tokio::fs::metadata(cache_dir).await.is_ok() {
            match tokio::fs::remove_dir_all(cache_dir).await {
                Ok(_) => info!("已清除缓存目录: {}", cache_dir),
                Err(e) => warn!("清除缓存目录失败 {}: {}", cache_dir, e),
            }
        }
    }

    Ok(())
}

/// 使用新架构处理
async fn process_with_new_architecture(config: Config, cli_args: CliArgs) -> Result<()> {
    info!("使用新架构处理视频流");

    // 创建服务工厂
    let domain_config = convert_to_domain_config(&config);
    let factory = ServiceFactory::new(domain_config.clone());

    // 创建视频处理服务
    let video_processor_service = factory.create_video_processor_service().await?;

    // 如果强制重新下载，清理临时文件
    if cli_args.force_redownload {
        video_processor_service
            .cleanup_temp_files(&domain_config)
            .await?;
    }

    // 处理每个URL
    for (i, m3u8_url) in config.m3u8_urls.iter().enumerate() {
        if m3u8_url.trim().is_empty() {
            continue;
        }

        info!(
            "处理 M3U8 [{}/{}]: {}",
            i + 1,
            config.m3u8_urls.len(),
            m3u8_url
        );

        match video_processor_service
            .process_video_stream(m3u8_url, &domain_config)
            .await
        {
            Ok(result) => {
                info!("处理成功: {}", result.output_file);
                info!(
                    "处理统计: 成功={}, 错误={}, 用时={:.2?}",
                    result.stats.completed_segments,
                    result.stats.failed_segments,
                    result.processing_time
                );
            }
            Err(e) => {
                error!("处理失败: {}", e);
                // 继续处理下一个URL
            }
        }
    }

    // 最终清理
    video_processor_service
        .cleanup_temp_files(&domain_config)
        .await?;

    Ok(())
}

/// 使用传统架构处理
async fn process_with_legacy_architecture(config: Config, cli_args: CliArgs) -> Result<()> {
    info!("使用传统架构处理视频流");

    // 创建向后兼容适配器
    let adapter = create_legacy_adapter(config.clone()).await?;

    // 如果强制重新下载，清理临时文件
    if cli_args.force_redownload {
        adapter.cleanup_temp_files().await?;
    }

    // 处理每个URL
    for (i, m3u8_url) in config.m3u8_urls.iter().enumerate() {
        if m3u8_url.trim().is_empty() {
            continue;
        }

        info!(
            "处理 M3U8 [{}/{}]: {}",
            i + 1,
            config.m3u8_urls.len(),
            m3u8_url
        );

        match adapter.process_m3u8_url(m3u8_url).await {
            Ok(result) => {
                info!("处理成功: {}", result.output_file);
                info!(
                    "处理统计: 成功={}, 错误={}, 用时={:.2?}",
                    result.stats.completed_segments,
                    result.stats.failed_segments,
                    result.processing_time
                );
            }
            Err(e) => {
                error!("处理失败: {}", e);
                // 继续处理下一个URL
            }
        }
    }

    // 最终清理
    adapter.cleanup_temp_files().await?;

    Ok(())
}

/// 转换配置格式
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
