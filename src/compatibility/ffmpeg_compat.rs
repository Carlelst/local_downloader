/// FFmpeg功能兼容性包装器
///
/// 提供与现有FFmpeg API兼容的接口，内部使用新的FFmpeg处理器实现
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Child;

use crate::domain::services::{FFmpegHandler, HardwareAccelerator, ProcessingConfig};
use crate::infrastructure::DefaultFFmpegHandler;
use crate::Config;

/// 兼容性FFmpeg启动函数 - 使用新架构实现
///
/// 该函数保持与原始 `start_ffmpeg` 函数相同的签名，
/// 但内部使用新的FFmpeg处理器实现
pub async fn start_ffmpeg_new(config: &Config) -> Result<(Child, String)> {
    // 创建新的FFmpeg处理器
    let handler = DefaultFFmpegHandler::new();

    // 转换配置到新的处理配置格式
    let processing_config = convert_config_to_processing_config(config, &handler).await?;

    // 启动FFmpeg进程
    let ffmpeg_process = handler.start_process(&processing_config).await?;

    // 提取Child进程（需要一些不安全的操作来兼容旧API）
    let output_path = ffmpeg_process.output_path.to_string_lossy().to_string();

    // 注意：这里我们需要创建一个兼容的Child对象
    // 由于新架构使用了不同的进程管理方式，我们需要创建一个代理
    let child = create_compatible_child_process(&processing_config).await?;

    Ok((child, output_path))
}

/// 转换旧配置到新的处理配置
async fn convert_config_to_processing_config(
    config: &Config,
    handler: &DefaultFFmpegHandler,
) -> Result<ProcessingConfig> {
    // 检测硬件加速
    let hw_accelerators = handler.detect_hardware_acceleration().await;
    let hardware_accel = if config.use_hw_accel && !hw_accelerators.is_empty() {
        // 选择第一个可用的硬件加速器（除了None）
        hw_accelerators
            .into_iter()
            .find(|accel| *accel != HardwareAccelerator::None)
            .or(Some(HardwareAccelerator::None))
    } else {
        Some(HardwareAccelerator::None)
    };

    // 创建自定义参数
    let mut custom_params = HashMap::new();

    // 添加时间戳处理参数
    custom_params.insert("-avoid_negative_ts".to_string(), "make_zero".to_string());
    custom_params.insert("-max_interleave_delta".to_string(), "0".to_string());
    custom_params.insert("-fflags".to_string(), "+genpts+igndts".to_string());

    // 添加输出优化参数
    custom_params.insert("-movflags".to_string(), "+faststart".to_string());
    custom_params.insert("-shortest".to_string(), "".to_string());

    // 设置日志级别
    let log_level = match config.log_level.as_str() {
        "debug" => "info",
        "info" => "warning",
        "warn" | "warning" => "warning",
        "error" => "error",
        _ => "warning",
    };
    custom_params.insert("-loglevel".to_string(), log_level.to_string());

    Ok(ProcessingConfig {
        video_codec: config.video_codec.clone(),
        audio_codec: config.audio_codec.clone(),
        video_bitrate: config.video_bitrate.clone(),
        audio_bitrate: config.audio_bitrate.clone(),
        hardware_accel,
        thread_count: if config.thread_count > 0 {
            config.thread_count
        } else {
            // 自动检测CPU核心数
            let cores = num_cpus::get();
            std::cmp::max(1, cores.saturating_sub(1)) as i32
        },
        output_format: config.video_format.clone(),
        custom_params,
    })
}

/// 创建兼容的Child进程
///
/// 注意：这是一个简化的实现，主要用于保持API兼容性
async fn create_compatible_child_process(config: &ProcessingConfig) -> Result<Child> {
    use rand::{distributions::Alphanumeric, Rng};
    use std::process::Stdio;
    use tokio::process::Command;

    // 生成输出文件名
    let random_filename: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    let output_filename = format!("{}.{}", random_filename, config.output_format);
    let output_path = PathBuf::from("output").join(&output_filename);

    // 确保输出目录存在
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut cmd = Command::new("ffmpeg");

    // 基本参数
    cmd.arg("-y"); // 覆盖输出文件

    // 硬件加速参数
    if let Some(ref hw_accel) = config.hardware_accel {
        add_hardware_accel_args(&mut cmd, hw_accel);
    }

    // 输入参数
    cmd.arg("-f").arg("mpegts").arg("-i").arg("pipe:0");

    // 线程数
    if config.thread_count > 0 {
        cmd.arg("-threads").arg(config.thread_count.to_string());
    }

    // 视频编码参数
    cmd.arg("-c:v").arg(&config.video_codec);
    if let Some(ref bitrate) = config.video_bitrate {
        cmd.arg("-b:v").arg(bitrate);
    }

    // 音频编码参数
    cmd.arg("-c:a").arg(&config.audio_codec);
    if let Some(ref bitrate) = config.audio_bitrate {
        cmd.arg("-b:a").arg(bitrate);
    }

    // 自定义参数
    for (key, value) in &config.custom_params {
        cmd.arg(key);
        if !value.is_empty() {
            cmd.arg(value);
        }
    }

    // 输出文件
    cmd.arg(output_path.to_string_lossy().as_ref());

    // 启动进程
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}

/// 添加硬件加速参数
fn add_hardware_accel_args(cmd: &mut tokio::process::Command, hw_accel: &HardwareAccelerator) {
    match hw_accel {
        HardwareAccelerator::VideoToolbox => {
            cmd.arg("-hwaccel").arg("videotoolbox");
        }
        HardwareAccelerator::Nvenc => {
            cmd.arg("-hwaccel").arg("cuda");
        }
        HardwareAccelerator::Qsv => {
            cmd.arg("-hwaccel")
                .arg("qsv")
                .arg("-hwaccel_output_format")
                .arg("qsv");
        }
        HardwareAccelerator::Cuda => {
            cmd.arg("-hwaccel")
                .arg("cuda")
                .arg("-hwaccel_output_format")
                .arg("cuda");
        }
        HardwareAccelerator::Vaapi => {
            cmd.arg("-hwaccel")
                .arg("vaapi")
                .arg("-hwaccel_output_format")
                .arg("vaapi");
        }
        HardwareAccelerator::None => {
            // 不添加硬件加速参数
        }
    }
}

/// 检测可用的硬件加速器（兼容性函数）
///
/// 使用新的FFmpeg处理器检测硬件加速
pub async fn detect_hardware_acceleration() -> Vec<String> {
    let handler = DefaultFFmpegHandler::new();
    let accelerators = handler.detect_hardware_acceleration().await;

    accelerators
        .into_iter()
        .map(|accel| match accel {
            HardwareAccelerator::VideoToolbox => "videotoolbox".to_string(),
            HardwareAccelerator::Nvenc => "nvenc".to_string(),
            HardwareAccelerator::Qsv => "qsv".to_string(),
            HardwareAccelerator::Cuda => "cuda".to_string(),
            HardwareAccelerator::Vaapi => "vaapi".to_string(),
            HardwareAccelerator::None => "none".to_string(),
        })
        .collect()
}

/// 优化编码参数（兼容性函数）
///
/// 使用新的FFmpeg处理器优化编码参数
pub async fn optimize_encoding_parameters(
    input_codec: Option<&str>,
    resolution: Option<(u32, u32)>,
    bitrate: Option<u64>,
) -> Result<HashMap<String, String>> {
    let handler = DefaultFFmpegHandler::new();

    let input_info = crate::domain::services::InputInfo {
        video_codec: input_codec.map(|s| s.to_string()),
        audio_codec: None,
        resolution,
        frame_rate: None,
        bitrate,
        duration: None,
    };

    let params = handler.optimize_encoding_params(&input_info).await;

    let mut result = HashMap::new();
    result.insert("video_codec".to_string(), params.video_codec);
    result.insert("audio_codec".to_string(), params.audio_codec);
    result.insert("video_bitrate".to_string(), params.video_bitrate);
    result.insert("audio_bitrate".to_string(), params.audio_bitrate);
    result.insert("preset".to_string(), params.preset);

    if let Some(crf) = params.crf {
        result.insert("crf".to_string(), crf.to_string());
    }

    Ok(result)
}

/// 创建兼容的FFmpeg处理器
///
/// 该函数创建一个配置好的FFmpeg处理器，可以用于高级操作
pub fn create_compatible_ffmpeg_handler() -> Box<dyn FFmpegHandler> {
    Box::new(DefaultFFmpegHandler::new())
}

/// FFmpeg进程监控器
///
/// 提供进程监控的兼容性接口
pub struct FFmpegProcessMonitor {
    handler: DefaultFFmpegHandler,
}

impl FFmpegProcessMonitor {
    pub fn new() -> Self {
        Self {
            handler: DefaultFFmpegHandler::new(),
        }
    }

    /// 监控进程状态
    pub async fn monitor_process(&self, process_id: u32) -> Result<String> {
        // 创建临时的FFmpeg进程对象用于监控
        let temp_process = crate::domain::services::FFmpegProcess {
            process_id,
            input_pipe: crate::domain::services::SyncInputPipe::new(Box::new(tokio::io::sink())),
            output_path: PathBuf::from("temp"),
            config: ProcessingConfig {
                video_codec: "libx264".to_string(),
                audio_codec: "aac".to_string(),
                video_bitrate: None,
                audio_bitrate: None,
                hardware_accel: Some(HardwareAccelerator::None),
                thread_count: 4,
                output_format: "mp4".to_string(),
                custom_params: HashMap::new(),
            },
            start_time: std::time::SystemTime::now(),
        };

        let status = self.handler.monitor_process(&temp_process).await?;

        Ok(match status {
            crate::domain::services::ProcessStatus::Running => "running".to_string(),
            crate::domain::services::ProcessStatus::Completed => "completed".to_string(),
            crate::domain::services::ProcessStatus::Failed(msg) => format!("failed: {}", msg),
            crate::domain::services::ProcessStatus::Terminated => "terminated".to_string(),
        })
    }

    /// 终止进程
    pub async fn terminate_process(&self, process_id: u32) -> Result<()> {
        let temp_process = crate::domain::services::FFmpegProcess {
            process_id,
            input_pipe: crate::domain::services::SyncInputPipe::new(Box::new(tokio::io::sink())),
            output_path: PathBuf::from("temp"),
            config: ProcessingConfig {
                video_codec: "libx264".to_string(),
                audio_codec: "aac".to_string(),
                video_bitrate: None,
                audio_bitrate: None,
                hardware_accel: Some(HardwareAccelerator::None),
                thread_count: 4,
                output_format: "mp4".to_string(),
                custom_params: HashMap::new(),
            },
            start_time: std::time::SystemTime::now(),
        };

        self.handler.terminate_process(&temp_process).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hardware_acceleration_detection() {
        let accelerators = detect_hardware_acceleration().await;

        // 至少应该有"none"选项
        assert!(!accelerators.is_empty());
        assert!(accelerators.contains(&"none".to_string()));

        println!("检测到的硬件加速器: {:?}", accelerators);
    }

    #[tokio::test]
    async fn test_encoding_parameter_optimization() {
        let result = optimize_encoding_parameters(
            Some("h264"),
            Some((1920, 1080)),
            Some(5_000_000), // 5Mbps
        )
        .await;

        assert!(result.is_ok());
        let params = result.unwrap();

        assert!(params.contains_key("video_codec"));
        assert!(params.contains_key("audio_codec"));
        assert!(params.contains_key("video_bitrate"));

        println!("优化后的编码参数: {:?}", params);
    }

    #[tokio::test]
    async fn test_config_conversion() {
        let config = Config {
            m3u8_urls: vec![],
            png_urls: vec![],
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
            thread_count: 4,
            video_bitrate: Some("2M".to_string()),
            audio_bitrate: Some("128k".to_string()),
            chunk_size: 1024 * 1024,
            referer: String::new(),
            referers: std::collections::HashMap::new(),
            cookies: std::collections::HashMap::new(),
        };

        let handler = DefaultFFmpegHandler::new();
        let result = convert_config_to_processing_config(&config, &handler).await;

        assert!(result.is_ok());
        let processing_config = result.unwrap();

        assert_eq!(processing_config.video_codec, "libx264");
        assert_eq!(processing_config.audio_codec, "aac");
        assert_eq!(processing_config.video_bitrate, Some("2M".to_string()));
        assert_eq!(processing_config.audio_bitrate, Some("128k".to_string()));
        assert_eq!(processing_config.thread_count, 4);
        assert_eq!(processing_config.output_format, "mp4");
    }

    #[test]
    fn test_process_monitor() {
        let monitor = FFmpegProcessMonitor::new();

        // 测试监控器创建
        // 实际的进程监控需要真实的进程ID，这里只测试创建
        println!("FFmpeg进程监控器创建成功");
    }
}
