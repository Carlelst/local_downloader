/// 配置验证模块
/// 提供详细的配置参数验证和错误报告
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::domain::entities::Config;

/// 验证错误类型
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub severity: ValidationSeverity,
}

/// 验证错误严重程度
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationSeverity {
    Error,   // 阻止启动的错误
    Warning, // 警告但不阻止启动
    Info,    // 信息性提示
}

/// 验证结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationError>,
    pub infos: Vec<ValidationError>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            infos: Vec::new(),
        }
    }

    pub fn add_error(&mut self, field: &str, message: &str) {
        self.is_valid = false;
        self.errors.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
            severity: ValidationSeverity::Error,
        });
    }

    pub fn add_warning(&mut self, field: &str, message: &str) {
        self.warnings.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
            severity: ValidationSeverity::Warning,
        });
    }

    pub fn add_info(&mut self, field: &str, message: &str) {
        self.infos.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
            severity: ValidationSeverity::Info,
        });
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn total_issues(&self) -> usize {
        self.errors.len() + self.warnings.len() + self.infos.len()
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// 配置验证器trait
pub trait ConfigValidator: Send + Sync {
    /// 验证配置
    fn validate(&self, config: &Config) -> ValidationResult;

    /// 获取验证器名称
    fn name(&self) -> &str;

    /// 获取验证器描述
    fn description(&self) -> &str;
}

/// 基本参数验证器
pub struct BasicParameterValidator;

impl ConfigValidator for BasicParameterValidator {
    fn validate(&self, config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // 验证并发下载数
        if config.max_concurrent_downloads == 0 {
            result.add_error("max_concurrent_downloads", "最大并发下载数必须大于0");
        } else if config.max_concurrent_downloads > 100 {
            result.add_warning(
                "max_concurrent_downloads",
                "最大并发下载数超过100可能会导致性能问题",
            );
        } else if config.max_concurrent_downloads > 50 {
            result.add_info(
                "max_concurrent_downloads",
                "建议将并发数控制在50以内以获得最佳性能",
            );
        }

        // 验证重试次数
        if config.download_retries > 10 {
            result.add_warning("download_retries", "重试次数过多可能会导致长时间等待");
        }

        // 验证超时时间
        if config.segment_timeout == 0 {
            result.add_error("segment_timeout", "片段超时时间必须大于0");
        } else if config.segment_timeout < 5 {
            result.add_warning("segment_timeout", "超时时间过短可能导致下载失败");
        } else if config.segment_timeout > 300 {
            result.add_warning("segment_timeout", "超时时间过长可能影响响应性");
        }

        // 验证重试间隔
        if config.retry_interval == 0 {
            result.add_warning("retry_interval", "重试间隔为0可能导致服务器压力过大");
        } else if config.retry_interval > 60 {
            result.add_info("retry_interval", "重试间隔较长，可能影响下载速度");
        }

        // 验证缓冲区大小
        if config.buffer_size == 0 {
            result.add_error("buffer_size", "缓冲区大小必须大于0");
        } else if config.buffer_size < 1024 * 1024 {
            result.add_warning("buffer_size", "缓冲区大小过小可能影响性能");
        } else if config.buffer_size > 100 * 1024 * 1024 {
            result.add_warning("buffer_size", "缓冲区大小过大可能消耗过多内存");
        }

        // 验证块大小
        if config.chunk_size == 0 {
            result.add_error("chunk_size", "块大小必须大于0");
        } else if config.chunk_size < 64 * 1024 {
            result.add_warning("chunk_size", "块大小过小可能影响下载效率");
        } else if config.chunk_size > 10 * 1024 * 1024 {
            result.add_warning("chunk_size", "块大小过大可能影响内存使用");
        }

        // 验证线程数
        if config.thread_count < -1 {
            result.add_error("thread_count", "线程数不能小于-1");
        } else if config.thread_count > 32 {
            result.add_warning("thread_count", "线程数过多可能导致系统负载过高");
        }

        result
    }

    fn name(&self) -> &str {
        "BasicParameterValidator"
    }

    fn description(&self) -> &str {
        "验证基本的数值参数范围和有效性"
    }
}

/// 路径和文件验证器
pub struct PathValidator;

impl ConfigValidator for PathValidator {
    fn validate(&self, config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // 验证输出目录
        if config.output_directory.is_empty() {
            result.add_error("output_directory", "输出目录不能为空");
        } else {
            let output_path = Path::new(&config.output_directory);

            // 检查父目录是否存在
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    result.add_error(
                        "output_directory",
                        &format!("输出目录的父目录不存在: {}", parent.display()),
                    );
                }
            }

            // 检查路径是否为绝对路径
            if output_path.is_relative() {
                result.add_info("output_directory", "使用相对路径，建议使用绝对路径");
            }

            // 检查写入权限（简化检查）
            if output_path.exists()
                && output_path
                    .metadata()
                    .map(|m| m.permissions().readonly())
                    .unwrap_or(false)
            {
                result.add_error("output_directory", "输出目录没有写入权限");
            }
        }

        result
    }

    fn name(&self) -> &str {
        "PathValidator"
    }

    fn description(&self) -> &str {
        "验证文件路径和目录的有效性"
    }
}

/// 格式和编码验证器
pub struct FormatValidator;

impl ConfigValidator for FormatValidator {
    fn validate(&self, config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // 验证日志级别
        match config.log_level.to_lowercase().as_str() {
            "debug" | "info" | "warn" | "error" => {}
            _ => result.add_error(
                "log_level",
                &format!(
                    "无效的日志级别: {}，支持的级别: debug, info, warn, error",
                    config.log_level
                ),
            ),
        }

        // 验证视频格式
        match config.video_format.to_lowercase().as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "ts" | "flv" | "webm" => {}
            _ => result.add_error(
                "video_format",
                &format!(
                    "不支持的视频格式: {}，支持的格式: mp4, mkv, avi, mov, ts, flv, webm",
                    config.video_format
                ),
            ),
        }

        // 验证视频编解码器
        match config.video_codec.to_lowercase().as_str() {
            "copy" | "libx264" | "libx265" | "h264" | "h265" | "vp8" | "vp9" | "av1" => {}
            _ => result.add_warning(
                "video_codec",
                &format!("可能不支持的视频编解码器: {}", config.video_codec),
            ),
        }

        // 验证音频编解码器
        match config.audio_codec.to_lowercase().as_str() {
            "copy" | "aac" | "mp3" | "opus" | "vorbis" | "flac" | "ac3" => {}
            _ => result.add_warning(
                "audio_codec",
                &format!("可能不支持的音频编解码器: {}", config.audio_codec),
            ),
        }

        // 验证比特率格式
        if let Some(ref bitrate) = config.video_bitrate {
            if !Self::is_valid_bitrate(bitrate) {
                result.add_error(
                    "video_bitrate",
                    &format!(
                        "无效的视频比特率格式: {}，应该以k或M结尾，如: 5000k, 5M",
                        bitrate
                    ),
                );
            }
        }

        if let Some(ref bitrate) = config.audio_bitrate {
            if !Self::is_valid_bitrate(bitrate) {
                result.add_error(
                    "audio_bitrate",
                    &format!(
                        "无效的音频比特率格式: {}，应该以k或M结尾，如: 320k, 1M",
                        bitrate
                    ),
                );
            }
        }

        result
    }

    fn name(&self) -> &str {
        "FormatValidator"
    }

    fn description(&self) -> &str {
        "验证媒体格式、编解码器和比特率的有效性"
    }
}

impl FormatValidator {
    fn is_valid_bitrate(bitrate: &str) -> bool {
        if bitrate.is_empty() {
            return false;
        }

        let bitrate = bitrate.trim();
        if bitrate.ends_with('k') || bitrate.ends_with('K') {
            bitrate[..bitrate.len() - 1].parse::<u32>().is_ok()
        } else if bitrate.ends_with('M') || bitrate.ends_with('m') {
            bitrate[..bitrate.len() - 1].parse::<u32>().is_ok()
        } else {
            false
        }
    }
}

/// URL验证器
pub struct UrlValidator;

impl ConfigValidator for UrlValidator {
    fn validate(&self, config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // 验证M3U8 URLs
        if config.m3u8_urls.is_empty() {
            result.add_warning("m3u8_urls", "没有配置M3U8 URL，可能需要在运行时提供");
        } else {
            for (index, url) in config.m3u8_urls.iter().enumerate() {
                if url.is_empty() {
                    result.add_error("m3u8_urls", &format!("第{}个URL为空", index + 1));
                    continue;
                }

                // 基本URL格式验证
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    result.add_warning(
                        "m3u8_urls",
                        &format!("第{}个URL可能不是有效的HTTP(S) URL: {}", index + 1, url),
                    );
                }

                // 检查是否是M3U8文件
                if !url.to_lowercase().contains(".m3u8") {
                    result.add_info(
                        "m3u8_urls",
                        &format!("第{}个URL可能不是M3U8文件: {}", index + 1, url),
                    );
                }
            }
        }

        result
    }

    fn name(&self) -> &str {
        "UrlValidator"
    }

    fn description(&self) -> &str {
        "验证URL格式和有效性"
    }
}

/// 兼容性验证器
pub struct CompatibilityValidator;

impl ConfigValidator for CompatibilityValidator {
    fn validate(&self, config: &Config) -> ValidationResult {
        let mut result = ValidationResult::new();

        // 检查硬件加速和编解码器的兼容性
        if config.use_hw_accel {
            match config.video_codec.to_lowercase().as_str() {
                "libx264" | "libx265" => {
                    result.add_info(
                        "video_codec",
                        "软件编码器与硬件加速可能不兼容，建议使用h264_nvenc等硬件编码器",
                    );
                }
                _ => {}
            }
        }

        // 检查格式和编解码器的兼容性
        match (
            config.video_format.to_lowercase().as_str(),
            config.video_codec.to_lowercase().as_str(),
        ) {
            ("mp4", "vp8") | ("mp4", "vp9") => {
                result.add_warning(
                    "video_codec",
                    "VP8/VP9编解码器与MP4格式兼容性有限，建议使用WebM格式",
                );
            }
            ("avi", "h265") => {
                result.add_warning(
                    "video_codec",
                    "H.265编解码器与AVI格式兼容性有限，建议使用MP4或MKV格式",
                );
            }
            _ => {}
        }

        // 检查音频编解码器和格式的兼容性
        match (
            config.video_format.to_lowercase().as_str(),
            config.audio_codec.to_lowercase().as_str(),
        ) {
            ("mp4", "vorbis") => {
                result.add_warning(
                    "audio_codec",
                    "Vorbis音频编解码器与MP4格式兼容性有限，建议使用AAC",
                );
            }
            ("avi", "opus") => {
                result.add_warning(
                    "audio_codec",
                    "Opus音频编解码器与AVI格式兼容性有限，建议使用MP3或AC3",
                );
            }
            _ => {}
        }

        result
    }

    fn name(&self) -> &str {
        "CompatibilityValidator"
    }

    fn description(&self) -> &str {
        "验证不同配置选项之间的兼容性"
    }
}

/// 配置验证管理器
pub struct ConfigValidationManager {
    validators: Vec<Box<dyn ConfigValidator>>,
}

impl ConfigValidationManager {
    pub fn new() -> Self {
        Self {
            validators: vec![
                Box::new(BasicParameterValidator),
                Box::new(PathValidator),
                Box::new(FormatValidator),
                Box::new(UrlValidator),
                Box::new(CompatibilityValidator),
            ],
        }
    }

    /// 添加自定义验证器
    pub fn add_validator(&mut self, validator: Box<dyn ConfigValidator>) {
        self.validators.push(validator);
    }

    /// 执行完整的配置验证
    pub fn validate_config(&self, config: &Config) -> ValidationResult {
        let mut combined_result = ValidationResult::new();

        for validator in &self.validators {
            let result = validator.validate(config);

            // 合并验证结果
            combined_result.errors.extend(result.errors);
            combined_result.warnings.extend(result.warnings);
            combined_result.infos.extend(result.infos);

            if !result.is_valid {
                combined_result.is_valid = false;
            }
        }

        combined_result
    }

    /// 获取详细的验证报告
    pub fn get_validation_report(&self, config: &Config) -> String {
        let result = self.validate_config(config);
        let mut report = String::new();

        report.push_str("=== 配置验证报告 ===\n\n");

        if result.is_valid && result.total_issues() == 0 {
            report.push_str("✅ 配置验证通过，没有发现问题。\n");
            return report;
        }

        if !result.errors.is_empty() {
            report.push_str("❌ 错误 (阻止启动):\n");
            for error in &result.errors {
                report.push_str(&format!("  • {}: {}\n", error.field, error.message));
            }
            report.push('\n');
        }

        if !result.warnings.is_empty() {
            report.push_str("⚠️  警告:\n");
            for warning in &result.warnings {
                report.push_str(&format!("  • {}: {}\n", warning.field, warning.message));
            }
            report.push('\n');
        }

        if !result.infos.is_empty() {
            report.push_str("ℹ️  信息:\n");
            for info in &result.infos {
                report.push_str(&format!("  • {}: {}\n", info.field, info.message));
            }
            report.push('\n');
        }

        report.push_str(&format!(
            "总计: {} 个错误, {} 个警告, {} 个信息\n",
            result.errors.len(),
            result.warnings.len(),
            result.infos.len()
        ));

        if !result.is_valid {
            report.push_str("\n❌ 配置验证失败，请修复上述错误后重试。\n");
        } else {
            report.push_str("\n✅ 配置验证通过，但有一些建议需要注意。\n");
        }

        report
    }

    /// 获取所有验证器信息
    pub fn get_validators_info(&self) -> Vec<(String, String)> {
        self.validators
            .iter()
            .map(|v| (v.name().to_string(), v.description().to_string()))
            .collect()
    }
}

impl Default for ConfigValidationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 配置验证工具函数
pub fn validate_config_with_details(config: &Config) -> Result<()> {
    let validator = ConfigValidationManager::new();
    let result = validator.validate_config(config);

    if !result.is_valid {
        let report = validator.get_validation_report(config);
        return Err(anyhow::anyhow!("配置验证失败:\n{}", report));
    }

    // 如果有警告或信息，记录到日志
    if result.has_warnings() || !result.infos.is_empty() {
        let report = validator.get_validation_report(config);
        log::warn!("配置验证通过但有建议:\n{}", report);
    }

    Ok(())
}

/// 快速配置验证（只检查错误）
pub fn validate_config_quick(config: &Config) -> Result<()> {
    let validator = ConfigValidationManager::new();
    let result = validator.validate_config(config);

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
