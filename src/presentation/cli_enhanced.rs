/// 增强的CLI接口
///
/// 提供现代化的命令行界面，支持新旧架构切换
use anyhow::Result;
use clap::{Arg, ArgAction, Command};
use std::path::PathBuf;

/// CLI配置结构
#[derive(Debug, Clone)]
pub struct EnhancedCliConfig {
    pub use_new_architecture: bool,
    pub clear_cache: bool,
    pub force_redownload: bool,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub use_copy_mode: bool,
    pub disable_hw_accel: bool,
    pub config_file: Option<PathBuf>,
    pub urls: Vec<String>,
    pub output_directory: Option<PathBuf>,
    pub max_concurrent: Option<usize>,
    pub log_level: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub video_format: Option<String>,
    pub thread_count: Option<i32>,
    pub validate_segments: bool,
    pub buffer_size: Option<usize>,
    pub chunk_size: Option<usize>,
    pub segment_timeout: Option<u64>,
    pub download_retries: Option<u8>,
    pub retry_interval: Option<u64>,
}

impl Default for EnhancedCliConfig {
    fn default() -> Self {
        Self {
            use_new_architecture: false,
            clear_cache: false,
            force_redownload: false,
            video_bitrate: None,
            audio_bitrate: None,
            use_copy_mode: false,
            disable_hw_accel: false,
            config_file: None,
            urls: Vec::new(),
            output_directory: None,
            max_concurrent: None,
            log_level: None,
            video_codec: None,
            audio_codec: None,
            video_format: None,
            thread_count: None,
            validate_segments: false,
            buffer_size: None,
            chunk_size: None,
            segment_timeout: None,
            download_retries: None,
            retry_interval: None,
        }
    }
}

/// 增强的CLI接口
pub struct EnhancedCli {
    app: Command,
}

impl EnhancedCli {
    /// 创建新的CLI接口
    pub fn new() -> Self {
        let app = Command::new("video_processor")
            .version("2.0.0")
            .author("Video Processor Team")
            .about("M3U8流媒体下载和处理工具 - 支持新旧架构")
            .long_about(
                "
一个强大的M3U8流媒体下载和处理工具，支持：
- 并发下载和智能重试
- AES加密流解密
- 硬件加速编码
- 断点续传
- 新旧架构兼容
            ",
            )
            // 架构选择
            .arg(
                Arg::new("new-arch")
                    .long("new-arch")
                    .help("使用新的分层架构（推荐）")
                    .action(ArgAction::SetTrue),
            )
            // 基本选项
            .arg(
                Arg::new("urls")
                    .help("要处理的M3U8 URL列表")
                    .action(ArgAction::Append)
                    .value_name("URL"),
            )
            .arg(
                Arg::new("config")
                    .short('c')
                    .long("config")
                    .help("配置文件路径")
                    .value_name("FILE"),
            )
            .arg(
                Arg::new("output")
                    .short('o')
                    .long("output")
                    .help("输出目录")
                    .value_name("DIR"),
            )
            // 下载选项
            .arg(
                Arg::new("concurrent")
                    .long("concurrent")
                    .help("最大并发下载数")
                    .value_name("NUM")
                    .value_parser(clap::value_parser!(usize)),
            )
            .arg(
                Arg::new("retries")
                    .long("retries")
                    .help("下载重试次数")
                    .value_name("NUM")
                    .value_parser(clap::value_parser!(u8)),
            )
            .arg(
                Arg::new("timeout")
                    .long("timeout")
                    .help("片段下载超时时间（秒）")
                    .value_name("SECS")
                    .value_parser(clap::value_parser!(u64)),
            )
            .arg(
                Arg::new("retry-interval")
                    .long("retry-interval")
                    .help("重试间隔时间（秒）")
                    .value_name("SECS")
                    .value_parser(clap::value_parser!(u64)),
            )
            // 编码选项
            .arg(
                Arg::new("video-codec")
                    .long("video-codec")
                    .help("视频编码器")
                    .value_name("CODEC")
                    .default_value("libx264"),
            )
            .arg(
                Arg::new("audio-codec")
                    .long("audio-codec")
                    .help("音频编码器")
                    .value_name("CODEC")
                    .default_value("aac"),
            )
            .arg(
                Arg::new("video-bitrate")
                    .long("video-bitrate")
                    .help("视频比特率 (例: 2M, 1500k)")
                    .value_name("RATE"),
            )
            .arg(
                Arg::new("audio-bitrate")
                    .long("audio-bitrate")
                    .help("音频比特率 (例: 128k, 320k)")
                    .value_name("RATE"),
            )
            .arg(
                Arg::new("video-format")
                    .long("format")
                    .help("输出视频格式")
                    .value_name("FORMAT")
                    .default_value("mp4"),
            )
            // 硬件加速选项
            .arg(
                Arg::new("copy")
                    .long("copy")
                    .help("使用copy模式，不重新编码视频")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("no-hw")
                    .long("no-hw")
                    .help("禁用硬件加速")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("threads")
                    .long("threads")
                    .help("编码线程数（0为自动）")
                    .value_name("NUM")
                    .value_parser(clap::value_parser!(i32)),
            )
            // 性能选项
            .arg(
                Arg::new("buffer-size")
                    .long("buffer-size")
                    .help("缓冲区大小（字节）")
                    .value_name("BYTES")
                    .value_parser(clap::value_parser!(usize)),
            )
            .arg(
                Arg::new("chunk-size")
                    .long("chunk-size")
                    .help("下载块大小（字节）")
                    .value_name("BYTES")
                    .value_parser(clap::value_parser!(usize)),
            )
            // 验证和调试选项
            .arg(
                Arg::new("validate")
                    .long("validate")
                    .help("验证片段完整性")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("log-level")
                    .long("log-level")
                    .help("日志级别")
                    .value_name("LEVEL")
                    .value_parser(["debug", "info", "warn", "error"])
                    .default_value("info"),
            )
            // 缓存和强制选项
            .arg(
                Arg::new("clear-cache")
                    .long("clear-cache")
                    .help("清除下载缓存")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("force")
                    .long("force")
                    .help("强制重新下载所有片段")
                    .action(ArgAction::SetTrue),
            )
            // 子命令
            .subcommand(
                Command::new("info").about("显示系统信息").arg(
                    Arg::new("hardware")
                        .long("hardware")
                        .help("显示硬件加速信息")
                        .action(ArgAction::SetTrue),
                ),
            )
            .subcommand(
                Command::new("test")
                    .about("测试功能")
                    .arg(Arg::new("url").help("测试URL").required(true))
                    .arg(
                        Arg::new("parse-only")
                            .long("parse-only")
                            .help("仅解析，不下载")
                            .action(ArgAction::SetTrue),
                    ),
            );

        Self { app }
    }

    /// 解析命令行参数
    pub fn parse(&mut self) -> Result<EnhancedCliConfig> {
        let matches = self.app.clone().try_get_matches()?;

        let mut config = EnhancedCliConfig::default();

        // 基本选项
        config.use_new_architecture = matches.get_flag("new-arch");
        config.clear_cache = matches.get_flag("clear-cache");
        config.force_redownload = matches.get_flag("force");
        config.use_copy_mode = matches.get_flag("copy");
        config.disable_hw_accel = matches.get_flag("no-hw");
        config.validate_segments = matches.get_flag("validate");

        // 文件路径
        if let Some(config_file) = matches.get_one::<String>("config") {
            config.config_file = Some(PathBuf::from(config_file));
        }

        if let Some(output_dir) = matches.get_one::<String>("output") {
            config.output_directory = Some(PathBuf::from(output_dir));
        }

        // URL列表
        if let Some(urls) = matches.get_many::<String>("urls") {
            config.urls = urls.cloned().collect();
        }

        // 数值选项
        config.max_concurrent = matches.get_one::<usize>("concurrent").copied();
        config.download_retries = matches.get_one::<u8>("retries").copied();
        config.segment_timeout = matches.get_one::<u64>("timeout").copied();
        config.retry_interval = matches.get_one::<u64>("retry-interval").copied();
        config.thread_count = matches.get_one::<i32>("threads").copied();
        config.buffer_size = matches.get_one::<usize>("buffer-size").copied();
        config.chunk_size = matches.get_one::<usize>("chunk-size").copied();

        // 字符串选项
        config.video_codec = matches.get_one::<String>("video-codec").cloned();
        config.audio_codec = matches.get_one::<String>("audio-codec").cloned();
        config.video_bitrate = matches.get_one::<String>("video-bitrate").cloned();
        config.audio_bitrate = matches.get_one::<String>("audio-bitrate").cloned();
        config.video_format = matches.get_one::<String>("video-format").cloned();
        config.log_level = matches.get_one::<String>("log-level").cloned();

        Ok(config)
    }

    /// 处理子命令
    pub async fn handle_subcommands(&mut self) -> Result<bool> {
        let matches = self.app.clone().try_get_matches()?;

        match matches.subcommand() {
            Some(("info", sub_matches)) => {
                self.handle_info_command(sub_matches).await?;
                Ok(true)
            }
            Some(("test", sub_matches)) => {
                self.handle_test_command(sub_matches).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// 处理info子命令
    async fn handle_info_command(&self, matches: &clap::ArgMatches) -> Result<()> {
        println!("视频处理器系统信息");
        println!("==================");

        // 显示版本信息
        println!("版本: 2.0.0");
        println!("架构: 分层架构 + 传统兼容");

        // 显示系统信息
        println!("CPU核心数: {}", num_cpus::get());
        println!("操作系统: {}", std::env::consts::OS);
        println!("架构: {}", std::env::consts::ARCH);

        // 显示硬件加速信息
        if matches.get_flag("hardware") {
            println!("\n硬件加速信息:");
            println!("--------------");

            use crate::compatibility::detect_hardware_acceleration;
            let accelerators = detect_hardware_acceleration().await;

            if accelerators.is_empty() {
                println!("未检测到硬件加速器");
            } else {
                for accel in accelerators {
                    println!("✓ {}", accel);
                }
            }
        }

        Ok(())
    }

    /// 处理test子命令
    async fn handle_test_command(&self, matches: &clap::ArgMatches) -> Result<()> {
        let url = matches.get_one::<String>("url").unwrap();
        let parse_only = matches.get_flag("parse-only");

        println!("测试URL: {}", url);

        if parse_only {
            println!("仅解析模式");

            // 使用新架构解析M3U8
            use crate::application::ServiceFactory;
            use crate::domain::entities::Config as DomainConfig;

            let config = DomainConfig::default();
            let factory = ServiceFactory::new(config);
            let parser = factory.create_m3u8_parser()?;

            match parser.parse(url).await {
                Ok(playlist) => {
                    println!("✓ 解析成功");
                    println!("  片段数量: {}", playlist.segments.len());
                    println!("  是否加密: {}", playlist.encryption.is_some());
                    if let Some(encryption) = &playlist.encryption {
                        println!("  加密方法: {:?}", encryption.method);
                    }
                }
                Err(e) => {
                    println!("✗ 解析失败: {}", e);
                }
            }
        } else {
            println!("完整测试模式（暂未实现）");
        }

        Ok(())
    }

    /// 显示使用示例
    pub fn print_examples(&self) {
        println!("使用示例:");
        println!("=========");
        println!();
        println!("1. 使用新架构下载视频:");
        println!(
            "   video_processor --new-arch --output ./downloads https://example.com/playlist.m3u8"
        );
        println!();
        println!("2. 使用copy模式（不重新编码）:");
        println!("   video_processor --copy --no-hw https://example.com/stream.m3u8");
        println!();
        println!("3. 设置自定义参数:");
        println!("   video_processor --video-bitrate 2M --concurrent 4 --retries 5 URL");
        println!();
        println!("4. 显示系统信息:");
        println!("   video_processor info --hardware");
        println!();
        println!("5. 测试URL解析:");
        println!("   video_processor test --parse-only https://example.com/test.m3u8");
    }
}

impl Default for EnhancedCli {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_creation() {
        let cli = EnhancedCli::new();
        assert!(cli.app.get_name() == "video_processor");
    }

    #[test]
    fn test_default_config() {
        let config = EnhancedCliConfig::default();
        assert!(!config.use_new_architecture);
        assert!(!config.clear_cache);
        assert!(config.urls.is_empty());
    }
}
