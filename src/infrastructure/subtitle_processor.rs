/// 字幕处理器的基础设施层实现
///
/// 该模块提供字幕文件下载、解析、转换和嵌入功能
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use regex::Regex;
use std::time::Duration;
use tokio::fs;

use crate::domain::services::{
    Subtitle, SubtitleEntry, SubtitleFile, SubtitleFormat, SubtitleMetadata, SubtitleProcessor,
};
use crate::infrastructure::HttpClient;

/// 默认字幕处理器实现
pub struct DefaultSubtitleProcessor {
    http_client: Box<dyn HttpClient + Send + Sync>,
}

impl DefaultSubtitleProcessor {
    /// 创建新的字幕处理器实例
    pub fn new(http_client: Box<dyn HttpClient + Send + Sync>) -> Self {
        Self { http_client }
    }

    /// 检测字幕格式
    pub fn detect_format(&self, content: &[u8]) -> SubtitleFormat {
        let text = String::from_utf8_lossy(content);
        let text_lower = text.to_lowercase();

        if text_lower.contains("webvtt") || text_lower.starts_with("webvtt") {
            SubtitleFormat::Vtt
        } else if text_lower.contains("[script info]")
            || text_lower.contains("!: this file was created by")
        {
            if text_lower.contains("scripttype: v4.00+") {
                SubtitleFormat::Ass
            } else {
                SubtitleFormat::Ssa
            }
        } else if text_lower.contains("<tt ") || text_lower.contains("<ttml") {
            SubtitleFormat::Ttml
        } else if text_lower.contains("scenarist_scc") || text.starts_with("Scenarist_SCC") {
            SubtitleFormat::Scc
        } else if Regex::new(r"^\d+\s*\n\d{2}:\d{2}:\d{2},\d{3}\s*-->\s*\d{2}:\d{2}:\d{2},\d{3}")
            .unwrap()
            .is_match(&text)
        {
            SubtitleFormat::Srt
        } else {
            SubtitleFormat::Unknown
        }
    }

    /// 解析SRT格式字幕
    fn parse_srt(&self, content: &str) -> Result<Vec<SubtitleEntry>> {
        let mut entries = Vec::new();
        let blocks: Vec<&str> = content.split("\n\n").collect();

        for block in blocks {
            let lines: Vec<&str> = block.trim().lines().collect();
            if lines.len() < 3 {
                continue;
            }

            // 解析索引
            let index = lines[0].trim().parse::<usize>().unwrap_or(0);

            // 解析时间轴
            let time_line = lines[1];
            let time_regex = Regex::new(
                r"(\d{2}):(\d{2}):(\d{2}),(\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2}),(\d{3})",
            )
            .unwrap();

            if let Some(captures) = time_regex.captures(time_line) {
                let start_time = Duration::from_millis(
                    captures[1].parse::<u64>().unwrap_or(0) * 3600000
                        + captures[2].parse::<u64>().unwrap_or(0) * 60000
                        + captures[3].parse::<u64>().unwrap_or(0) * 1000
                        + captures[4].parse::<u64>().unwrap_or(0),
                );

                let end_time = Duration::from_millis(
                    captures[5].parse::<u64>().unwrap_or(0) * 3600000
                        + captures[6].parse::<u64>().unwrap_or(0) * 60000
                        + captures[7].parse::<u64>().unwrap_or(0) * 1000
                        + captures[8].parse::<u64>().unwrap_or(0),
                );

                // 解析文本内容
                let text = lines[2..].join("\n");

                entries.push(SubtitleEntry {
                    index,
                    start_time,
                    end_time,
                    text,
                    style: None,
                });
            }
        }

        Ok(entries)
    }

    /// 解析VTT格式字幕
    fn parse_vtt(&self, content: &str) -> Result<Vec<SubtitleEntry>> {
        let mut entries = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;
        let mut index = 1;

        // 跳过WEBVTT头部
        while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].contains("-->") {
            i += 1;
        }

        while i < lines.len() {
            // 跳过空行
            while i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }

            if i >= lines.len() {
                break;
            }

            // 检查是否是时间轴行
            let time_regex = Regex::new(
                r"(\d{2}):(\d{2}):(\d{2})\.(\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2})\.(\d{3})",
            )
            .unwrap();

            if let Some(captures) = time_regex.captures(lines[i]) {
                let start_time = Duration::from_millis(
                    captures[1].parse::<u64>().unwrap_or(0) * 3600000
                        + captures[2].parse::<u64>().unwrap_or(0) * 60000
                        + captures[3].parse::<u64>().unwrap_or(0) * 1000
                        + captures[4].parse::<u64>().unwrap_or(0),
                );

                let end_time = Duration::from_millis(
                    captures[5].parse::<u64>().unwrap_or(0) * 3600000
                        + captures[6].parse::<u64>().unwrap_or(0) * 60000
                        + captures[7].parse::<u64>().unwrap_or(0) * 1000
                        + captures[8].parse::<u64>().unwrap_or(0),
                );

                i += 1;

                // 收集文本行
                let mut text_lines = Vec::new();
                while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].contains("-->") {
                    text_lines.push(lines[i]);
                    i += 1;
                }

                let text = text_lines.join("\n");

                entries.push(SubtitleEntry {
                    index,
                    start_time,
                    end_time,
                    text,
                    style: None,
                });

                index += 1;
            } else {
                i += 1;
            }
        }

        Ok(entries)
    }

    /// 格式化时间为SRT格式
    fn format_srt_time(&self, duration: Duration) -> String {
        let total_ms = duration.as_millis() as u64;
        let hours = total_ms / 3600000;
        let minutes = (total_ms % 3600000) / 60000;
        let seconds = (total_ms % 60000) / 1000;
        let milliseconds = total_ms % 1000;

        format!(
            "{:02}:{:02}:{:02},{:03}",
            hours, minutes, seconds, milliseconds
        )
    }

    /// 格式化时间为VTT格式
    fn format_vtt_time(&self, duration: Duration) -> String {
        let total_ms = duration.as_millis() as u64;
        let hours = total_ms / 3600000;
        let minutes = (total_ms % 3600000) / 60000;
        let seconds = (total_ms % 60000) / 1000;
        let milliseconds = total_ms % 1000;

        format!(
            "{:02}:{:02}:{:02}.{:03}",
            hours, minutes, seconds, milliseconds
        )
    }

    /// 转换为SRT格式
    fn convert_to_srt(&self, subtitle: &Subtitle) -> Result<Vec<u8>> {
        let mut output = String::new();

        for entry in &subtitle.entries {
            output.push_str(&format!("{}\n", entry.index));
            output.push_str(&format!(
                "{} --> {}\n",
                self.format_srt_time(entry.start_time),
                self.format_srt_time(entry.end_time)
            ));
            output.push_str(&format!("{}\n\n", entry.text));
        }

        Ok(output.into_bytes())
    }

    /// 转换为VTT格式
    fn convert_to_vtt(&self, subtitle: &Subtitle) -> Result<Vec<u8>> {
        let mut output = String::from("WEBVTT\n\n");

        for entry in &subtitle.entries {
            output.push_str(&format!(
                "{} --> {}\n",
                self.format_vtt_time(entry.start_time),
                self.format_vtt_time(entry.end_time)
            ));
            output.push_str(&format!("{}\n\n", entry.text));
        }

        Ok(output.into_bytes())
    }
}

#[async_trait]
impl SubtitleProcessor for DefaultSubtitleProcessor {
    async fn download_subtitle(&self, url: &str) -> Result<SubtitleFile> {
        debug!("下载字幕文件: {}", url);

        let response = self
            .http_client
            .get(url, Some(Duration::from_secs(30)))
            .await
            .context("下载字幕文件失败")?;

        if response.status < 200 || response.status >= 300 {
            return Err(anyhow::anyhow!(
                "下载字幕文件失败，状态码: {}",
                response.status
            ));
        }

        let content = response.body;
        let format = self.detect_format(&content);

        info!(
            "成功下载字幕文件，大小: {} 字节，格式: {:?}",
            content.len(),
            format
        );

        Ok(SubtitleFile {
            url: url.to_string(),
            content,
            format,
            language: None,
            encoding: None,
        })
    }

    async fn parse_subtitle(&self, content: &[u8], format: SubtitleFormat) -> Result<Subtitle> {
        debug!("解析字幕文件，格式: {:?}", format);

        let text = String::from_utf8_lossy(content);

        let entries = match format {
            SubtitleFormat::Srt => self.parse_srt(&text)?,
            SubtitleFormat::Vtt => self.parse_vtt(&text)?,
            SubtitleFormat::Ass | SubtitleFormat::Ssa => {
                warn!("ASS/SSA格式解析尚未完全实现，使用基础解析");
                Vec::new() // 简化实现
            }
            SubtitleFormat::Ttml => {
                warn!("TTML格式解析尚未实现");
                Vec::new()
            }
            SubtitleFormat::Scc => {
                warn!("SCC格式解析尚未实现");
                Vec::new()
            }
            SubtitleFormat::Unknown => {
                return Err(anyhow::anyhow!("未知的字幕格式"));
            }
        };

        let metadata = SubtitleMetadata {
            total_entries: entries.len(),
            duration: entries.last().map(|e| e.end_time),
            ..Default::default()
        };

        info!("成功解析字幕，共 {} 条字幕", entries.len());

        Ok(Subtitle {
            entries,
            format,
            language: None,
            metadata,
        })
    }

    async fn convert_subtitle(
        &self,
        subtitle: &Subtitle,
        target_format: SubtitleFormat,
    ) -> Result<Vec<u8>> {
        debug!("转换字幕格式: {:?} -> {:?}", subtitle.format, target_format);

        match target_format {
            SubtitleFormat::Srt => self.convert_to_srt(subtitle),
            SubtitleFormat::Vtt => self.convert_to_vtt(subtitle),
            _ => {
                warn!("目标格式 {:?} 转换尚未实现", target_format);
                Err(anyhow::anyhow!("不支持的目标格式: {:?}", target_format))
            }
        }
    }

    async fn sync_subtitle(&self, subtitle: &mut Subtitle, offset: Duration) -> Result<()> {
        debug!("同步字幕时间轴，偏移: {:?}", offset);

        for entry in &mut subtitle.entries {
            if offset.as_millis() > 0 {
                entry.start_time += offset;
                entry.end_time += offset;
            } else {
                let abs_offset = Duration::from_millis(offset.as_millis() as u64);
                if entry.start_time > abs_offset {
                    entry.start_time -= abs_offset;
                } else {
                    entry.start_time = Duration::from_millis(0);
                }

                if entry.end_time > abs_offset {
                    entry.end_time -= abs_offset;
                } else {
                    entry.end_time = Duration::from_millis(0);
                }
            }
        }

        info!("成功同步 {} 条字幕的时间轴", subtitle.entries.len());
        Ok(())
    }

    fn validate_subtitle(&self, subtitle: &Subtitle) -> Result<()> {
        debug!("验证字幕完整性");

        if subtitle.entries.is_empty() {
            return Err(anyhow::anyhow!("字幕文件为空"));
        }

        for (i, entry) in subtitle.entries.iter().enumerate() {
            if entry.start_time >= entry.end_time {
                return Err(anyhow::anyhow!(
                    "字幕条目 {} 的时间轴无效: 开始时间 >= 结束时间",
                    i + 1
                ));
            }

            if entry.text.trim().is_empty() {
                warn!("字幕条目 {} 的文本为空", i + 1);
            }
        }

        // 检查时间轴重叠
        for i in 1..subtitle.entries.len() {
            if subtitle.entries[i].start_time < subtitle.entries[i - 1].end_time {
                warn!("字幕条目 {} 和 {} 存在时间轴重叠", i, i + 1);
            }
        }

        info!("字幕验证完成，共 {} 条字幕", subtitle.entries.len());
        Ok(())
    }

    async fn embed_subtitle(
        &self,
        video_path: &str,
        subtitle: &Subtitle,
        output_path: &str,
    ) -> Result<()> {
        debug!("嵌入字幕到视频: {} -> {}", video_path, output_path);

        // 创建临时字幕文件
        let temp_subtitle_path = format!("{}.srt", output_path);
        let subtitle_content = self.convert_to_srt(subtitle)?;
        fs::write(&temp_subtitle_path, subtitle_content)
            .await
            .context("写入临时字幕文件失败")?;

        // 使用FFmpeg嵌入字幕
        let output = tokio::process::Command::new("ffmpeg")
            .arg("-i")
            .arg(video_path)
            .arg("-i")
            .arg(&temp_subtitle_path)
            .arg("-c")
            .arg("copy")
            .arg("-c:s")
            .arg("mov_text") // MP4字幕格式
            .arg("-y")
            .arg(output_path)
            .output()
            .await
            .context("执行FFmpeg命令失败")?;

        // 清理临时文件
        let _ = fs::remove_file(&temp_subtitle_path).await;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("FFmpeg嵌入字幕失败: {}", error_msg));
        }

        info!("成功嵌入字幕到视频文件");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::DefaultHttpClient;

    #[tokio::test]
    async fn test_srt_parsing() {
        let http_client = Box::new(DefaultHttpClient::new(30).unwrap());
        let processor = DefaultSubtitleProcessor::new(http_client);

        let srt_content = r#"1
00:00:01,000 --> 00:00:04,000
Hello, world!

2
00:00:05,000 --> 00:00:08,000
This is a test subtitle.
"#;

        let subtitle = processor
            .parse_subtitle(srt_content.as_bytes(), SubtitleFormat::Srt)
            .await
            .unwrap();

        assert_eq!(subtitle.entries.len(), 2);
        assert_eq!(subtitle.entries[0].text, "Hello, world!");
        assert_eq!(subtitle.entries[1].text, "This is a test subtitle.");
    }

    #[tokio::test]
    async fn test_vtt_parsing() {
        let http_client = Box::new(DefaultHttpClient::new(30).unwrap());
        let processor = DefaultSubtitleProcessor::new(http_client);

        let vtt_content = r#"WEBVTT

00:00:01.000 --> 00:00:04.000
Hello, world!

00:00:05.000 --> 00:00:08.000
This is a test subtitle.
"#;

        let subtitle = processor
            .parse_subtitle(vtt_content.as_bytes(), SubtitleFormat::Vtt)
            .await
            .unwrap();

        assert_eq!(subtitle.entries.len(), 2);
        assert_eq!(subtitle.entries[0].text, "Hello, world!");
        assert_eq!(subtitle.entries[1].text, "This is a test subtitle.");
    }

    #[tokio::test]
    async fn test_format_detection() {
        let http_client = Box::new(DefaultHttpClient::new(30).unwrap());
        let processor = DefaultSubtitleProcessor::new(http_client);

        let srt_content = b"1\n00:00:01,000 --> 00:00:04,000\nHello";
        assert_eq!(processor.detect_format(srt_content), SubtitleFormat::Srt);

        let vtt_content = b"WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello";
        assert_eq!(processor.detect_format(vtt_content), SubtitleFormat::Vtt);
    }

    #[tokio::test]
    async fn test_subtitle_conversion() {
        let http_client = Box::new(DefaultHttpClient::new(30).unwrap());
        let processor = DefaultSubtitleProcessor::new(http_client);

        let subtitle = Subtitle {
            entries: vec![SubtitleEntry {
                index: 1,
                start_time: Duration::from_millis(1000),
                end_time: Duration::from_millis(4000),
                text: "Hello, world!".to_string(),
                style: None,
            }],
            format: SubtitleFormat::Srt,
            language: None,
            metadata: SubtitleMetadata::default(),
        };

        let srt_content = processor
            .convert_subtitle(&subtitle, SubtitleFormat::Srt)
            .await
            .unwrap();
        let srt_text = String::from_utf8(srt_content).unwrap();
        assert!(srt_text.contains("00:00:01,000 --> 00:00:04,000"));

        let vtt_content = processor
            .convert_subtitle(&subtitle, SubtitleFormat::Vtt)
            .await
            .unwrap();
        let vtt_text = String::from_utf8(vtt_content).unwrap();
        assert!(vtt_text.contains("WEBVTT"));
        assert!(vtt_text.contains("00:00:01.000 --> 00:00:04.000"));
    }
}
