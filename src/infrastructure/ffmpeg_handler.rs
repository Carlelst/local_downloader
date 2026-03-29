/// FFmpeg处理器的基础设施层实现
///
/// 该模块提供FFmpeg进程管理、硬件加速检测和编码参数优化功能
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

use crate::domain::services::{
    EncodingParams, FFmpegHandler, FFmpegProcess, HardwareAccelerator, InputInfo, ProcessStatus,
    ProcessingConfig,
};

/// 默认FFmpeg处理器实现
pub struct DefaultFFmpegHandler {
    /// 活跃进程列表
    active_processes: Arc<Mutex<HashMap<u32, Arc<Mutex<Child>>>>>,
    /// 硬件加速器缓存
    hw_accel_cache: Arc<Mutex<Option<Vec<HardwareAccelerator>>>>,
    /// 进程监控间隔
    monitor_interval: Duration,
}

impl DefaultFFmpegHandler {
    /// 创建新的FFmpeg处理器实例
    pub fn new() -> Self {
        Self {
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            hw_accel_cache: Arc::new(Mutex::new(None)),
            monitor_interval: Duration::from_secs(1),
        }
    }

    /// 设置进程监控间隔
    pub fn with_monitor_interval(mut self, interval: Duration) -> Self {
        self.monitor_interval = interval;
        self
    }

    /// 检测单个硬件加速器
    async fn test_hardware_accelerator(&self, accel: &str) -> bool {
        debug!("测试硬件加速器: {}", accel);

        let result = timeout(Duration::from_secs(10), async {
            Command::new("ffmpeg")
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .arg("-hwaccel")
                .arg(accel)
                .arg("-f")
                .arg("lavfi")
                .arg("-i")
                .arg("nullsrc=s=320x240:d=0.1")
                .arg("-an")
                .arg("-f")
                .arg("null")
                .arg("-")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
        })
        .await;

        match result {
            Ok(Ok(status)) => {
                let success = status.success();
                if success {
                    info!("硬件加速器 {} 可用", accel);
                } else {
                    debug!("硬件加速器 {} 不可用", accel);
                }
                success
            }
            Ok(Err(e)) => {
                debug!("测试硬件加速器 {} 时出错: {}", accel, e);
                false
            }
            Err(_) => {
                debug!("测试硬件加速器 {} 超时", accel);
                false
            }
        }
    }

    /// 构建FFmpeg命令参数
    fn build_ffmpeg_command(&self, config: &ProcessingConfig, output_path: &PathBuf) -> Command {
        let mut cmd = Command::new("ffmpeg");

        // 基本参数
        cmd.arg("-y"); // 覆盖输出文件
        cmd.arg("-hide_banner"); // 隐藏版权信息

        // 硬件加速参数
        if let Some(ref hw_accel) = config.hardware_accel {
            self.add_hardware_accel_args(&mut cmd, hw_accel);
        }

        // 输入参数
        cmd.arg("-f").arg("mpegts").arg("-i").arg("pipe:0");

        // 线程数
        if config.thread_count > 0 {
            cmd.arg("-threads").arg(config.thread_count.to_string());
        }

        // 时间戳处理
        cmd.arg("-avoid_negative_ts")
            .arg("make_zero")
            .arg("-max_interleave_delta")
            .arg("0")
            .arg("-fflags")
            .arg("+genpts+igndts");

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
            cmd.arg(key).arg(value);
        }

        // 输出优化
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg("-shortest");

        // 输出文件
        cmd.arg(output_path.to_string_lossy().as_ref());

        // 日志级别
        cmd.arg("-loglevel").arg("warning");

        cmd
    }

    /// 添加硬件加速参数
    fn add_hardware_accel_args(&self, cmd: &mut Command, hw_accel: &HardwareAccelerator) {
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

    /// 生成输出文件路径
    fn generate_output_path(&self, config: &ProcessingConfig) -> PathBuf {
        use rand::{distributions::Alphanumeric, Rng};

        let random_name: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        let filename = format!("{}.{}", random_name, config.output_format);
        PathBuf::from("output").join(filename)
    }

    /// 启动进程监控任务
    async fn start_process_monitor(&self, process_id: u32, child: Arc<Mutex<Child>>) {
        let processes = Arc::clone(&self.active_processes);
        let monitor_interval = self.monitor_interval;

        tokio::spawn(async move {
            loop {
                sleep(monitor_interval).await;

                let mut child_guard = child.lock().await;
                match child_guard.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            info!("FFmpeg进程 {} 正常完成", process_id);
                        } else {
                            warn!(
                                "FFmpeg进程 {} 异常退出，状态码: {:?}",
                                process_id,
                                status.code()
                            );
                        }

                        // 从活跃进程列表中移除
                        processes.lock().await.remove(&process_id);
                        break;
                    }
                    Ok(None) => {
                        // 进程仍在运行
                        debug!("FFmpeg进程 {} 运行中", process_id);
                    }
                    Err(e) => {
                        error!("检查FFmpeg进程 {} 状态时出错: {}", process_id, e);
                        break;
                    }
                }
            }
        });
    }
}

#[async_trait]
impl FFmpegHandler for DefaultFFmpegHandler {
    async fn start_process(&self, config: &ProcessingConfig) -> Result<FFmpegProcess> {
        debug!("启动FFmpeg进程，配置: {:?}", config);

        // 生成输出路径
        let output_path = self.generate_output_path(config);

        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("创建输出目录失败")?;
        }

        // 构建命令
        let mut cmd = self.build_ffmpeg_command(config, &output_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        debug!("FFmpeg命令: {:?}", cmd);

        // 启动进程
        let mut child = cmd.spawn().context("启动FFmpeg进程失败")?;
        let process_id = child.id().unwrap_or(0);

        // 获取stdin
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("无法获取FFmpeg进程的stdin"))?;

        // 将进程添加到活跃列表
        let child_arc = Arc::new(Mutex::new(child));
        self.active_processes
            .lock()
            .await
            .insert(process_id, Arc::clone(&child_arc));

        // 启动监控
        self.start_process_monitor(process_id, child_arc).await;

        info!(
            "FFmpeg进程 {} 启动成功，输出: {}",
            process_id,
            output_path.display()
        );

        Ok(FFmpegProcess {
            process_id,
            input_pipe: crate::domain::services::SyncInputPipe::new(Box::new(stdin)),
            output_path,
            config: config.clone(),
            start_time: SystemTime::now(),
        })
    }

    async fn detect_hardware_acceleration(&self) -> Vec<HardwareAccelerator> {
        // 检查缓存
        {
            let cache = self.hw_accel_cache.lock().await;
            if let Some(ref cached) = *cache {
                debug!("使用缓存的硬件加速器列表");
                return cached.clone();
            }
        }

        info!("检测可用的硬件加速器...");
        let mut available = Vec::new();

        // 定义要测试的加速器及其对应的枚举值
        let accelerators = [
            ("videotoolbox", HardwareAccelerator::VideoToolbox),
            ("cuda", HardwareAccelerator::Cuda),
            ("nvenc", HardwareAccelerator::Nvenc),
            ("qsv", HardwareAccelerator::Qsv),
            ("vaapi", HardwareAccelerator::Vaapi),
        ];

        for (name, accel_type) in accelerators.iter() {
            if self.test_hardware_accelerator(name).await {
                available.push(accel_type.clone());
            }
        }

        // 总是添加软件编码选项
        available.push(HardwareAccelerator::None);

        info!("检测到 {} 个可用的硬件加速器", available.len());

        // 缓存结果
        {
            let mut cache = self.hw_accel_cache.lock().await;
            *cache = Some(available.clone());
        }

        available
    }

    async fn optimize_encoding_params(&self, input_info: &InputInfo) -> EncodingParams {
        debug!("优化编码参数，输入信息: {:?}", input_info);

        let mut params = EncodingParams {
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            video_bitrate: "2M".to_string(),
            audio_bitrate: "128k".to_string(),
            preset: "medium".to_string(),
            crf: Some(23),
            additional_params: Vec::new(),
        };

        // 根据输入分辨率调整比特率
        if let Some((width, height)) = input_info.resolution {
            let pixels = width * height;
            params.video_bitrate = match pixels {
                0..=307200 => "500k".to_string(),      // 480p及以下
                307201..=921600 => "1M".to_string(),   // 720p
                921601..=2073600 => "2M".to_string(),  // 1080p
                2073601..=8294400 => "4M".to_string(), // 4K
                _ => "8M".to_string(),                 // 8K及以上
            };

            debug!(
                "根据分辨率 {}x{} 设置视频比特率: {}",
                width, height, params.video_bitrate
            );
        }

        // 根据输入编解码器优化
        if let Some(ref codec) = input_info.video_codec {
            match codec.to_lowercase().as_str() {
                "h264" | "avc1" | "avc" => {
                    params.video_codec = "libx264".to_string();
                    params.preset = "fast".to_string();
                    params.crf = Some(23);
                }
                "hevc" | "h265" | "hvc1" => {
                    params.video_codec = "libx265".to_string();
                    params.preset = "medium".to_string();
                    params.crf = Some(28); // HEVC可以使用更高的CRF
                }
                "vp8" => {
                    params.video_codec = "libvpx".to_string();
                    params.preset = "good".to_string();
                    params.crf = Some(10); // VP8使用不同的质量范围
                    params.additional_params.push("-cpu-used".to_string());
                    params.additional_params.push("2".to_string());
                }
                "vp9" => {
                    params.video_codec = "libvpx-vp9".to_string();
                    params.preset = "good".to_string();
                    params.crf = Some(30);
                    params.additional_params.push("-cpu-used".to_string());
                    params.additional_params.push("2".to_string());
                }
                "av1" => {
                    params.video_codec = "libaom-av1".to_string();
                    params.preset = "6".to_string(); // AV1使用数字预设
                    params.crf = Some(30);
                    params.additional_params.push("-cpu-used".to_string());
                    params.additional_params.push("4".to_string());
                }
                "mpeg2" | "mpeg2video" => {
                    params.video_codec = "mpeg2video".to_string();
                    params.preset = "medium".to_string();
                    params.crf = None; // MPEG2不支持CRF
                }
                "mpeg4" => {
                    params.video_codec = "mpeg4".to_string();
                    params.preset = "medium".to_string();
                    params.crf = None;
                }
                "copy" => {
                    params.video_codec = "copy".to_string();
                    params.crf = None;
                }
                _ => {
                    // 保持默认设置
                    debug!("未知的视频编解码器: {}，使用默认设置", codec);
                }
            }
        }

        // 根据输入音频编解码器优化
        if let Some(ref codec) = input_info.audio_codec {
            match codec.to_lowercase().as_str() {
                "aac" | "mp4a" => {
                    params.audio_codec = "aac".to_string();
                    params.audio_bitrate = "128k".to_string();
                }
                "mp3" | "mp3float" => {
                    params.audio_codec = "libmp3lame".to_string();
                    params.audio_bitrate = "192k".to_string();
                }
                "opus" => {
                    params.audio_codec = "libopus".to_string();
                    params.audio_bitrate = "96k".to_string(); // Opus在低比特率下表现更好
                }
                "vorbis" => {
                    params.audio_codec = "libvorbis".to_string();
                    params.audio_bitrate = "128k".to_string();
                }
                "flac" => {
                    params.audio_codec = "flac".to_string();
                    params.audio_bitrate = "".to_string(); // FLAC是无损的，不需要比特率
                }
                "pcm" | "pcm_s16le" | "pcm_s24le" => {
                    params.audio_codec = "pcm_s16le".to_string();
                    params.audio_bitrate = "".to_string(); // PCM不需要比特率设置
                }
                "ac3" => {
                    params.audio_codec = "ac3".to_string();
                    params.audio_bitrate = "384k".to_string();
                }
                "eac3" => {
                    params.audio_codec = "eac3".to_string();
                    params.audio_bitrate = "640k".to_string();
                }
                "dts" => {
                    params.audio_codec = "dca".to_string();
                    params.audio_bitrate = "1536k".to_string();
                }
                "copy" => {
                    params.audio_codec = "copy".to_string();
                    params.audio_bitrate = "".to_string();
                }
                _ => {
                    // 保持默认设置
                    debug!("未知的音频编解码器: {}，使用默认设置", codec);
                }
            }
        }

        // 根据输入比特率调整
        if let Some(input_bitrate) = input_info.bitrate {
            let target_bitrate = (input_bitrate as f64 * 0.8) as u64; // 目标比特率为输入的80%
            params.video_bitrate = format!("{}k", target_bitrate / 1000);
            debug!("根据输入比特率调整目标比特率: {}", params.video_bitrate);
        }

        // 根据帧率优化
        if let Some(frame_rate) = input_info.frame_rate {
            if frame_rate > 60.0 {
                // 高帧率视频，可能需要更高的比特率
                if let Ok(current_bitrate) =
                    params.video_bitrate.trim_end_matches('k').parse::<u32>()
                {
                    let adjusted_bitrate = (current_bitrate as f64 * 1.5) as u32;
                    params.video_bitrate = format!("{}k", adjusted_bitrate);
                    debug!(
                        "高帧率 ({} fps) 调整比特率至: {}",
                        frame_rate, params.video_bitrate
                    );
                }
            } else if frame_rate < 24.0 {
                // 低帧率视频，可以降低比特率
                if let Ok(current_bitrate) =
                    params.video_bitrate.trim_end_matches('k').parse::<u32>()
                {
                    let adjusted_bitrate = (current_bitrate as f64 * 0.7) as u32;
                    params.video_bitrate = format!("{}k", adjusted_bitrate);
                    debug!(
                        "低帧率 ({} fps) 调整比特率至: {}",
                        frame_rate, params.video_bitrate
                    );
                }
            }
        }

        // 质量预设优化
        self.apply_quality_presets(&mut params, input_info);

        debug!("优化后的编码参数: {:?}", params);
        params
    }

    async fn monitor_process(&self, process: &FFmpegProcess) -> Result<ProcessStatus> {
        let processes = self.active_processes.lock().await;

        if let Some(child_arc) = processes.get(&process.process_id) {
            let mut child = child_arc.lock().await;

            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        Ok(ProcessStatus::Completed)
                    } else {
                        let error_msg = format!("进程退出码: {:?}", status.code());
                        Ok(ProcessStatus::Failed(error_msg))
                    }
                }
                Ok(None) => Ok(ProcessStatus::Running),
                Err(e) => {
                    let error_msg = format!("监控进程时出错: {}", e);
                    Ok(ProcessStatus::Failed(error_msg))
                }
            }
        } else {
            Ok(ProcessStatus::Terminated)
        }
    }

    async fn terminate_process(&self, process: &FFmpegProcess) -> Result<()> {
        info!("终止FFmpeg进程 {}", process.process_id);

        let mut processes = self.active_processes.lock().await;

        if let Some(child_arc) = processes.remove(&process.process_id) {
            let mut child = child_arc.lock().await;

            // 尝试优雅终止
            if let Err(e) = child.start_kill() {
                warn!("优雅终止进程失败: {}, 尝试强制终止", e);
            }

            // 等待进程结束
            match timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    info!(
                        "FFmpeg进程 {} 已终止，状态: {:?}",
                        process.process_id, status
                    );
                }
                Ok(Err(e)) => {
                    error!("等待进程终止时出错: {}", e);
                }
                Err(_) => {
                    warn!("等待进程终止超时，进程可能仍在运行");
                }
            }
        } else {
            debug!("进程 {} 不在活跃列表中，可能已经终止", process.process_id);
        }

        Ok(())
    }
}

impl DefaultFFmpegHandler {
    /// 应用质量预设
    fn apply_quality_presets(&self, params: &mut EncodingParams, input_info: &InputInfo) {
        // 根据输入内容类型调整预设
        if let Some(duration) = input_info.duration {
            if duration.as_secs() > 3600 {
                // 长视频，优先考虑文件大小
                match params.video_codec.as_str() {
                    "libx264" => {
                        params.preset = "slow".to_string();
                        params.crf = Some(26); // 稍微降低质量以减小文件大小
                    }
                    "libx265" => {
                        params.preset = "slow".to_string();
                        params.crf = Some(30);
                    }
                    _ => {}
                }
            } else if duration.as_secs() < 300 {
                // 短视频，优先考虑质量
                match params.video_codec.as_str() {
                    "libx264" => {
                        params.preset = "slower".to_string();
                        params.crf = Some(20); // 更高质量
                    }
                    "libx265" => {
                        params.preset = "slower".to_string();
                        params.crf = Some(25);
                    }
                    _ => {}
                }
            }
        }

        // 根据分辨率调整预设
        if let Some((width, height)) = input_info.resolution {
            let pixels = width * height;
            if pixels >= 3840 * 2160 {
                // 4K及以上，使用更快的预设以减少编码时间
                match params.video_codec.as_str() {
                    "libx264" => params.preset = "fast".to_string(),
                    "libx265" => params.preset = "fast".to_string(),
                    _ => {}
                }
            }
        }
    }
}

impl Default for DefaultFFmpegHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_hardware_acceleration_detection() {
        let handler = DefaultFFmpegHandler::new();
        let accelerators = handler.detect_hardware_acceleration().await;

        // 至少应该有软件编码选项
        assert!(!accelerators.is_empty());
        assert!(accelerators.contains(&HardwareAccelerator::None));
    }

    #[tokio::test]
    async fn test_encoding_params_optimization() {
        let handler = DefaultFFmpegHandler::new();

        let input_info = InputInfo {
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            resolution: Some((1920, 1080)),
            frame_rate: Some(30.0),
            bitrate: Some(5_000_000), // 5Mbps
            duration: Some(Duration::from_secs(3600)),
        };

        let params = handler.optimize_encoding_params(&input_info).await;

        assert_eq!(params.video_codec, "libx264");
        assert_eq!(params.audio_codec, "aac");
        assert!(!params.video_bitrate.is_empty());
        assert!(!params.audio_bitrate.is_empty());
    }

    #[test]
    fn test_output_path_generation() {
        let handler = DefaultFFmpegHandler::new();
        let config = ProcessingConfig {
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            video_bitrate: None,
            audio_bitrate: None,
            hardware_accel: None,
            thread_count: 4,
            output_format: "mp4".to_string(),
            custom_params: HashMap::new(),
        };

        let path = handler.generate_output_path(&config);

        assert!(path.to_string_lossy().ends_with(".mp4"));
        assert!(path.to_string_lossy().contains("output"));
    }
}
