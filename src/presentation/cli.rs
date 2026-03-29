/// CLI接口模块
/// 处理命令行参数解析和用户交互
use anyhow::Result;
use std::path::PathBuf;

/// CLI配置结构
#[derive(Debug, Clone)]
pub struct CliConfig {
    pub m3u8_urls: Vec<String>,
    pub output_directory: PathBuf,
    pub config_file: Option<PathBuf>,
    pub force_redownload: bool,
    pub clear_cache: bool,
    pub disable_hardware_accel: bool,
    pub video_bitrate: Option<String>,
    pub copy_mode: bool,
    pub log_level: Option<String>,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            m3u8_urls: Vec::new(),
            output_directory: PathBuf::from("."),
            config_file: None,
            force_redownload: false,
            clear_cache: false,
            disable_hardware_accel: false,
            video_bitrate: None,
            copy_mode: false,
            log_level: None,
        }
    }
}

/// CLI接口trait
pub trait CliInterface {
    /// 解析命令行参数
    fn parse_args(&self) -> Result<CliConfig>;

    /// 显示帮助信息
    fn show_help(&self);

    /// 显示版本信息
    fn show_version(&self);
}

/// 默认CLI实现
pub struct DefaultCliInterface;

impl CliInterface for DefaultCliInterface {
    fn parse_args(&self) -> Result<CliConfig> {
        let mut config = CliConfig::default();
        let args: Vec<String> = std::env::args().collect();

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--help" | "-h" => {
                    self.show_help();
                    std::process::exit(0);
                }
                "--version" | "-v" => {
                    self.show_version();
                    std::process::exit(0);
                }
                "--force" => {
                    config.force_redownload = true;
                }
                "--clear-cache" => {
                    config.clear_cache = true;
                }
                "--copy" => {
                    config.copy_mode = true;
                }
                "--no-hw" => {
                    config.disable_hardware_accel = true;
                }
                arg if arg.starts_with("--bitrate=") => {
                    config.video_bitrate = Some(arg.split('=').nth(1).unwrap_or("2M").to_string());
                }
                arg if arg.starts_with("--output=") => {
                    config.output_directory = PathBuf::from(arg.split('=').nth(1).unwrap_or("."));
                }
                arg if arg.starts_with("--config=") => {
                    config.config_file = Some(PathBuf::from(arg.split('=').nth(1).unwrap()));
                }
                arg if arg.starts_with("--log-level=") => {
                    config.log_level = Some(arg.split('=').nth(1).unwrap_or("info").to_string());
                }
                arg if !arg.starts_with("--") => {
                    // 假设是M3U8 URL
                    config.m3u8_urls.push(arg.to_string());
                }
                _ => {
                    // 忽略未知参数
                }
            }
            i += 1;
        }

        Ok(config)
    }

    fn show_help(&self) {
        println!("Video Processor - M3U8流媒体下载和处理工具");
        println!();
        println!("用法:");
        println!("  video_processor [选项] <M3U8_URL>...");
        println!();
        println!("选项:");
        println!("  --help, -h              显示帮助信息");
        println!("  --version, -v           显示版本信息");
        println!("  --output=<DIR>          设置输出目录 (默认: .)");
        println!("  --config=<FILE>         指定配置文件");
        println!("  --force                 强制重新下载所有片段");
        println!("  --clear-cache           清除下载缓存");
        println!("  --copy                  使用copy模式，不重新编码");
        println!("  --no-hw                 禁用硬件加速");
        println!("  --bitrate=<RATE>        设置视频比特率 (如: 2M)");
        println!("  --log-level=<LEVEL>     设置日志级别 (debug|info|warn|error)");
    }

    fn show_version(&self) {
        println!("Video Processor v{}", env!("CARGO_PKG_VERSION"));
    }
}
