/// 配置管理模块 - 表示层配置接口
use anyhow::Result;
use std::path::Path;
/// 配置显示trait
pub trait ConfigPresentation {
    /// 显示当前配置
    fn display_config(&self, config: &crate::domain::entities::Config) -> Result<()>;

    /// 验证配置并显示验证结果
    fn display_validation_result(&self, result: &Result<()>) -> Result<()>;

    /// 显示配置加载状态
    fn display_load_status(&self, source: &str, success: bool) -> Result<()>;
}

/// 默认配置显示实现
pub struct DefaultConfigPresentation;

impl ConfigPresentation for DefaultConfigPresentation {
    fn display_config(&self, config: &crate::domain::entities::Config) -> Result<()> {
        println!("当前配置:");
        println!("  M3U8 URLs: {:?}", config.m3u8_urls);
        println!("  最大并发下载数: {}", config.max_concurrent_downloads);
        println!("  下载重试次数: {}", config.download_retries);
        println!("  片段超时: {}s", config.segment_timeout);
        println!("  输出目录: {}", config.output_directory);
        println!("  日志级别: {}", config.log_level);
        println!("  视频格式: {}", config.video_format);
        println!("  视频编解码器: {}", config.video_codec);
        println!("  音频编解码器: {}", config.audio_codec);
        println!("  使用硬件加速: {}", config.use_hw_accel);
        Ok(())
    }

    fn display_validation_result(&self, result: &Result<()>) -> Result<()> {
        match result {
            Ok(_) => println!("✓ 配置验证通过"),
            Err(e) => println!("✗ 配置验证失败: {}", e),
        }
        Ok(())
    }

    fn display_load_status(&self, source: &str, success: bool) -> Result<()> {
        if success {
            println!("✓ 成功从 {} 加载配置", source);
        } else {
            println!("✗ 从 {} 加载配置失败", source);
        }
        Ok(())
    }
}
