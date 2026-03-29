/// M3U8解析器基础设施实现
/// 提供增强的M3U8格式兼容性和错误诊断
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use regex::Regex;
use std::collections::HashSet;
use std::time::{Duration, SystemTime};
use url::Url;

use crate::domain::entities::{
    EncryptionInfo, EncryptionMethod, Playlist, PlaylistMetadata, Segment, SegmentMetadata,
};
use crate::domain::services::{M3U8Parser, PlaylistType};
use crate::infrastructure::http::HttpClient;

/// 默认M3U8解析器实现
pub struct DefaultM3U8Parser {
    http_client: Box<dyn HttpClient>,
    max_recursion_depth: usize,
    request_timeout: Duration,
    supported_versions: Vec<u32>,
}

impl DefaultM3U8Parser {
    pub fn new(http_client: Box<dyn HttpClient>) -> Self {
        Self {
            http_client,
            max_recursion_depth: 5,
            request_timeout: Duration::from_secs(10),
            supported_versions: vec![1, 2, 3, 4, 5, 6, 7],
        }
    }

    pub fn with_config(
        http_client: Box<dyn HttpClient>,
        max_recursion_depth: usize,
        request_timeout: Duration,
    ) -> Self {
        Self {
            http_client,
            max_recursion_depth,
            request_timeout,
            supported_versions: vec![1, 2, 3, 4, 5, 6, 7],
        }
    }

    /// 递归解析M3U8，支持主播放列表
    fn parse_with_depth<'a>(
        &'a self,
        url: &'a str,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Playlist>> + Send + 'a>> {
        Box::pin(async move {
            if depth > self.max_recursion_depth {
                return Err(anyhow!(
                    "M3U8递归层级过深 (>{}), 可能存在循环引用: {}",
                    self.max_recursion_depth,
                    url
                ));
            }

            if url.trim().is_empty() {
                return Err(anyhow!("M3U8 URL不能为空"));
            }

            info!("解析M3U8播放列表 (深度: {}): {}", depth, url);

            // 获取M3U8内容
            let content = self
                .fetch_m3u8_content(url)
                .await
                .with_context(|| format!("获取M3U8内容失败: {}", url))?;

            // 检测播放列表类型
            let playlist_type = self.detect_playlist_type(&content)?;

            match playlist_type {
                PlaylistType::Master => {
                    info!("检测到主播放列表，查找最佳子播放列表");
                    let sub_url = self.select_best_variant(&content, url)?;
                    self.parse_with_depth(&sub_url, depth + 1).await
                }
                PlaylistType::Media => {
                    info!("解析媒体播放列表");
                    self.parse_media_playlist(&content, url).await
                }
                PlaylistType::Unknown => Err(anyhow!("无法识别的播放列表格式: {}", url)),
            }
        })
    }

    /// 获取M3U8内容
    async fn fetch_m3u8_content(&self, url: &str) -> Result<String> {
        // 特殊处理：如果是图片文件，直接返回URL作为内容
        if self.is_image_file(url) {
            info!("检测到图片文件，直接返回URL: {}", url);
            return Ok(format!("#EXTM3U\n#EXTINF:10.0,\n{}", url));
        }

        let response = self
            .http_client
            .get(url, Some(self.request_timeout))
            .await
            .with_context(|| format!("HTTP请求失败: {}", url))?;

        if !response.is_success() {
            return Err(anyhow!(
                "HTTP错误: {} {}, URL: {}",
                response.status_code(),
                response.status_text().unwrap_or("Unknown"),
                url
            ));
        }

        let content = response
            .text()
            .await
            .with_context(|| format!("读取响应内容失败: {}", url))?;

        if content.trim().is_empty() {
            return Err(anyhow!("M3U8内容为空: {}", url));
        }

        debug!("M3U8内容长度: {} 字节", content.len());
        Ok(content)
    }

    /// 检查是否为图片文件
    fn is_image_file(&self, url: &str) -> bool {
        let lower_url = url.to_lowercase();
        lower_url.ends_with(".jpg")
            || lower_url.ends_with(".jpeg")
            || lower_url.ends_with(".png")
            || lower_url.ends_with(".gif")
            || lower_url.ends_with(".webp")
    }

    /// 从主播放列表中选择最佳变体
    fn select_best_variant(&self, content: &str, base_url: &str) -> Result<String> {
        let base_url =
            Url::parse(base_url).with_context(|| format!("无效的基础URL: {}", base_url))?;

        // 匹配EXT-X-STREAM-INF行和对应的URL
        let stream_inf_re = Regex::new(r"(?m)^#EXT-X-STREAM-INF:([^\r\n]*)\r?\n([^\r\n#]+)")
            .map_err(|e| anyhow!("正则表达式编译失败: {}", e))?;

        let mut variants = Vec::new();

        for captures in stream_inf_re.captures_iter(content) {
            let attributes = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let url_path = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");

            if url_path.is_empty() {
                continue;
            }

            let full_url = if url_path.starts_with("http") {
                url_path.to_string()
            } else {
                base_url
                    .join(url_path)
                    .with_context(|| format!("URL拼接失败: {} + {}", base_url, url_path))?
                    .to_string()
            };

            // 解析带宽信息
            let bandwidth = self.extract_bandwidth(attributes).unwrap_or(0);
            let resolution = self.extract_resolution(attributes);

            variants.push(StreamVariant {
                url: full_url,
                bandwidth,
                resolution,
                attributes: attributes.to_string(),
            });
        }

        if variants.is_empty() {
            return Err(anyhow!("主播放列表中未找到任何变体"));
        }

        // 选择最高带宽的变体
        variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        let selected = &variants[0];

        info!(
            "选择变体: 带宽={}, 分辨率={:?}, URL={}",
            selected.bandwidth, selected.resolution, selected.url
        );

        Ok(selected.url.clone())
    }

    /// 提取带宽信息
    fn extract_bandwidth(&self, attributes: &str) -> Option<u64> {
        let bandwidth_re = Regex::new(r"BANDWIDTH=(\d+)").ok()?;
        bandwidth_re
            .captures(attributes)?
            .get(1)?
            .as_str()
            .parse()
            .ok()
    }

    /// 提取分辨率信息
    fn extract_resolution(&self, attributes: &str) -> Option<(u32, u32)> {
        let resolution_re = Regex::new(r"RESOLUTION=(\d+)x(\d+)").ok()?;
        let captures = resolution_re.captures(attributes)?;
        let width = captures.get(1)?.as_str().parse().ok()?;
        let height = captures.get(2)?.as_str().parse().ok()?;
        Some((width, height))
    }

    /// 解析媒体播放列表
    async fn parse_media_playlist(&self, content: &str, url: &str) -> Result<Playlist> {
        let base_url = Url::parse(url).with_context(|| format!("无效的播放列表URL: {}", url))?;

        // 解析播放列表元数据
        let metadata = self.parse_playlist_metadata(content)?;

        // 解析加密信息
        let encryption = self.parse_encryption_info(content, &base_url).await?;

        // 解析片段列表
        let segments = self.parse_segments(content, &base_url, &encryption)?;

        if segments.is_empty() {
            warn!("播放列表中未找到任何片段: {}", url);
        }

        info!(
            "解析完成: {} 个片段, 加密: {}, 版本: {:?}",
            segments.len(),
            encryption.is_some(),
            metadata.version
        );

        Ok(Playlist {
            url: url.to_string(),
            segments,
            encryption,
            metadata,
            created_at: SystemTime::now(),
        })
    }

    /// 解析播放列表元数据
    fn parse_playlist_metadata(&self, content: &str) -> Result<PlaylistMetadata> {
        let mut metadata = PlaylistMetadata::default();

        // 解析版本
        if let Some(captures) = Regex::new(r"#EXT-X-VERSION:(\d+)")?.captures(content) {
            if let Some(version_str) = captures.get(1) {
                metadata.version = version_str.as_str().parse().ok();
            }
        }

        // 解析目标持续时间
        if let Some(captures) = Regex::new(r"#EXT-X-TARGETDURATION:(\d+)")?.captures(content) {
            if let Some(duration_str) = captures.get(1) {
                metadata.target_duration = duration_str.as_str().parse::<f64>().ok();
            }
        }

        // 解析媒体序列号
        if let Some(captures) = Regex::new(r"#EXT-X-MEDIA-SEQUENCE:(\d+)")?.captures(content) {
            if let Some(seq_str) = captures.get(1) {
                metadata.media_sequence = seq_str.as_str().parse().ok();
            }
        }

        // 解析播放列表类型
        if content.contains("#EXT-X-PLAYLIST-TYPE:") {
            if let Some(captures) = Regex::new(r"#EXT-X-PLAYLIST-TYPE:([A-Z]+)")?.captures(content)
            {
                if let Some(type_str) = captures.get(1) {
                    metadata.playlist_type = Some(type_str.as_str().to_string());
                }
            }
        }

        // 检测是否为直播流
        metadata.is_live = !content.contains("#EXT-X-ENDLIST");

        // 验证版本兼容性
        if let Some(version) = metadata.version {
            if !self.supported_versions.contains(&version) {
                warn!(
                    "不支持的M3U8版本: {}, 支持的版本: {:?}",
                    version, self.supported_versions
                );
            }
        }

        Ok(metadata)
    }

    /// 解析加密信息
    async fn parse_encryption_info(
        &self,
        content: &str,
        base_url: &Url,
    ) -> Result<Option<EncryptionInfo>> {
        let key_re = Regex::new(
            r#"#EXT-X-KEY:METHOD=([A-Z0-9-]+)(?:,URI=(?:"([^"]+)"|'([^']+)'))?(?:,IV=(?:0x)?([0-9a-fA-F]+))?"#,
        )?;

        if let Some(captures) = key_re.captures(content) {
            let method_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");

            let method = match method_str {
                "AES-128" => EncryptionMethod::Aes128,
                "AES-256" => EncryptionMethod::Aes256,
                "SAMPLE-AES" => EncryptionMethod::SampleAes,
                "NONE" => EncryptionMethod::None,
                _ => {
                    warn!("未知的加密方法: {}", method_str);
                    return Ok(None);
                }
            };

            if method == EncryptionMethod::None {
                debug!("检测到未加密流 (METHOD=NONE)");
                return Ok(None);
            }

            // 获取密钥URL
            let key_url = captures.get(2).or_else(|| captures.get(3)).map(|m| {
                let url_str = m.as_str();
                if url_str.starts_with("http") {
                    url_str.to_string()
                } else {
                    base_url
                        .join(url_str)
                        .map(|u| u.to_string())
                        .unwrap_or_else(|_| {
                            warn!("无法解析密钥URL: {}", url_str);
                            url_str.to_string()
                        })
                }
            });

            // 解析IV
            let iv = captures
                .get(4)
                .map(|m| {
                    let iv_str = m.as_str();
                    match hex::decode(iv_str) {
                        Ok(iv_bytes) => {
                            if iv_bytes.len() != 16 {
                                warn!("IV长度异常: {} 字节，期望16字节", iv_bytes.len());
                                vec![0; 16] // 使用全零IV作为回退
                            } else {
                                iv_bytes
                            }
                        }
                        Err(e) => {
                            warn!("IV解码失败: {}, 原始值: {}", e, iv_str);
                            vec![0; 16] // 使用全零IV作为回退
                        }
                    }
                })
                .or_else(|| {
                    info!("未提供IV，使用默认全零IV");
                    Some(vec![0; 16])
                });

            // 下载密钥
            let key_data = if let Some(ref url) = key_url {
                Some(self.download_encryption_key(url).await?)
            } else if method != EncryptionMethod::None {
                return Err(anyhow!("加密方法为{}但未提供密钥URI", method_str));
            } else {
                None
            };

            Ok(Some(EncryptionInfo {
                method,
                key_url,
                key_data,
                iv,
            }))
        } else {
            debug!("未找到加密信息，假定未加密");
            Ok(None)
        }
    }

    /// 下载加密密钥
    async fn download_encryption_key(&self, key_url: &str) -> Result<Vec<u8>> {
        debug!("下载加密密钥: {}", key_url);

        let response = self
            .http_client
            .get(key_url, Some(Duration::from_secs(5)))
            .await
            .with_context(|| format!("获取密钥失败: {}", key_url))?;

        if !response.is_success() {
            return Err(anyhow!(
                "密钥请求失败，HTTP状态码: {}, URL: {}",
                response.status_code(),
                key_url
            ));
        }

        let key_bytes = response
            .bytes()
            .await
            .with_context(|| format!("读取密钥内容失败: {}", key_url))?;

        // 验证密钥长度（支持AES-128和AES-256）
        match key_bytes.len() {
            16 => {
                info!("成功获取AES-128密钥，长度: 16字节");
            }
            32 => {
                info!("成功获取AES-256密钥，长度: 32字节");
            }
            _ => {
                warn!(
                    "AES密钥长度异常: {} 字节，期望16字节(AES-128)或32字节(AES-256)",
                    key_bytes.len()
                );
            }
        }

        Ok(key_bytes)
    }

    /// 解析片段列表
    fn parse_segments(
        &self,
        content: &str,
        base_url: &Url,
        encryption: &Option<EncryptionInfo>,
    ) -> Result<Vec<Segment>> {
        let mut segments = Vec::new();
        let mut processed_urls = HashSet::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_duration = 0.0;
        let mut current_metadata = SegmentMetadata::default();
        let mut segment_index = 0;

        for (i, line) in lines.iter().enumerate() {
            let line = line.trim();

            if line.starts_with("#EXTINF:") {
                // 解析片段持续时间
                current_duration = self.parse_extinf_duration(line)?;

                // 查找下一行的URL
                if i + 1 < lines.len() {
                    let next_line = lines[i + 1].trim();
                    if !next_line.starts_with('#') && !next_line.is_empty() {
                        let segment_url = self.resolve_segment_url(next_line, base_url)?;

                        if !processed_urls.contains(&segment_url) {
                            processed_urls.insert(segment_url.clone());

                            segments.push(Segment {
                                index: segment_index,
                                url: segment_url,
                                duration: current_duration,
                                size: None, // 将在下载时确定
                                encryption: encryption.clone(),
                                metadata: current_metadata.clone(),
                            });

                            segment_index += 1;
                        }
                    }
                }
            } else if line.starts_with("#EXT-X-DISCONTINUITY") {
                current_metadata.discontinuity = true;
            } else if line.starts_with("#EXT-X-BYTERANGE:") {
                current_metadata.byte_range = self.parse_byte_range(line);
            } else if line.starts_with("#EXT-X-PROGRAM-DATE-TIME:") {
                current_metadata.program_date_time = self.parse_program_date_time(line);
            } else if !line.starts_with('#') && !line.is_empty() {
                // 直接的片段URL（没有EXTINF前缀）
                let segment_url = self.resolve_segment_url(line, base_url)?;

                if !processed_urls.contains(&segment_url) {
                    processed_urls.insert(segment_url.clone());

                    segments.push(Segment {
                        index: segment_index,
                        url: segment_url,
                        duration: current_duration.max(10.0), // 默认10秒
                        size: None,
                        encryption: encryption.clone(),
                        metadata: current_metadata.clone(),
                    });

                    segment_index += 1;
                }
            }

            // 重置元数据（除了discontinuity，它会影响后续片段）
            if !line.starts_with("#EXT-X-DISCONTINUITY") {
                current_metadata = SegmentMetadata {
                    discontinuity: current_metadata.discontinuity,
                    ..Default::default()
                };
            }
        }

        debug!("解析到 {} 个唯一片段", segments.len());
        Ok(segments)
    }

    /// 解析EXTINF持续时间
    fn parse_extinf_duration(&self, line: &str) -> Result<f64> {
        let duration_re = Regex::new(r"#EXTINF:([0-9.]+)")?;
        if let Some(captures) = duration_re.captures(line) {
            if let Some(duration_str) = captures.get(1) {
                return duration_str
                    .as_str()
                    .parse()
                    .with_context(|| format!("无法解析片段持续时间: {}", line));
            }
        }

        warn!("无法解析EXTINF持续时间，使用默认值10.0: {}", line);
        Ok(10.0)
    }

    /// 解析字节范围
    fn parse_byte_range(&self, line: &str) -> Option<(u64, u64)> {
        let range_re = Regex::new(r"#EXT-X-BYTERANGE:(\d+)(?:@(\d+))?").ok()?;
        let captures = range_re.captures(line)?;

        let length: u64 = captures.get(1)?.as_str().parse().ok()?;
        let offset: u64 = captures
            .get(2)
            .map(|m| m.as_str().parse().unwrap_or(0))
            .unwrap_or(0);

        Some((offset, offset + length - 1))
    }

    /// 解析程序日期时间
    fn parse_program_date_time(&self, _line: &str) -> Option<SystemTime> {
        // 简化实现，实际应该解析ISO 8601格式
        // 这里只是占位符实现
        None
    }

    /// 解析片段URL
    fn resolve_segment_url(&self, url_str: &str, base_url: &Url) -> Result<String> {
        let url_str = url_str.trim();

        if url_str.is_empty() {
            return Err(anyhow!("片段URL为空"));
        }

        if url_str.starts_with("http://") || url_str.starts_with("https://") {
            Ok(url_str.to_string())
        } else {
            base_url
                .join(url_str)
                .map(|u| u.to_string())
                .with_context(|| format!("URL拼接失败: {} + {}", base_url, url_str))
        }
    }
}

#[async_trait]
impl M3U8Parser for DefaultM3U8Parser {
    async fn parse(&self, url: &str) -> Result<Playlist> {
        self.parse_with_depth(url, 0).await
    }

    async fn resolve_segments(&self, playlist: &Playlist) -> Result<Vec<Segment>> {
        // 对于已解析的播放列表，直接返回片段列表
        Ok(playlist.segments.clone())
    }

    fn validate_playlist(&self, playlist: &Playlist) -> Result<()> {
        if playlist.url.is_empty() {
            return Err(anyhow!("播放列表URL为空"));
        }

        if playlist.segments.is_empty() {
            return Err(anyhow!("播放列表不包含任何片段"));
        }

        // 验证片段URL
        for (i, segment) in playlist.segments.iter().enumerate() {
            if segment.url.is_empty() {
                return Err(anyhow!("片段 {} 的URL为空", i));
            }

            if segment.duration <= 0.0 {
                return Err(anyhow!("片段 {} 的持续时间无效: {}", i, segment.duration));
            }
        }

        // 验证加密信息
        if let Some(ref encryption) = playlist.encryption {
            if encryption.method != EncryptionMethod::None {
                if encryption.key_data.is_none() {
                    return Err(anyhow!("加密播放列表缺少密钥数据"));
                }

                if let Some(ref key_data) = encryption.key_data {
                    if key_data.len() != 16 {
                        return Err(anyhow!("AES密钥长度无效: {} 字节", key_data.len()));
                    }
                }
            }
        }

        info!("播放列表验证通过: {} 个片段", playlist.segments.len());
        Ok(())
    }

    fn detect_playlist_type(&self, content: &str) -> Result<PlaylistType> {
        if !content.contains("#EXTM3U") {
            return Ok(PlaylistType::Unknown);
        }

        // 检查是否包含EXT-X-STREAM-INF（主播放列表标识）
        if content.contains("#EXT-X-STREAM-INF:") {
            return Ok(PlaylistType::Master);
        }

        // 检查是否包含EXTINF（媒体播放列表标识）
        if content.contains("#EXTINF:") {
            return Ok(PlaylistType::Media);
        }

        // 检查其他媒体播放列表标识
        if content.contains("#EXT-X-TARGETDURATION:")
            || content.contains("#EXT-X-MEDIA-SEQUENCE:")
            || content.contains("#EXT-X-ENDLIST")
        {
            return Ok(PlaylistType::Media);
        }

        Ok(PlaylistType::Unknown)
    }
}

/// 流变体信息
#[derive(Debug, Clone)]
struct StreamVariant {
    url: String,
    bandwidth: u64,
    resolution: Option<(u32, u32)>,
    attributes: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::http::MockHttpClient;

    #[tokio::test]
    async fn test_detect_playlist_type() {
        let parser = DefaultM3U8Parser::new(Box::new(MockHttpClient::new()));

        // 测试主播放列表
        let master_content = r#"
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=854x480
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720
high/index.m3u8
        "#;
        assert_eq!(
            parser.detect_playlist_type(master_content).unwrap(),
            PlaylistType::Master
        );

        // 测试媒体播放列表
        let media_content = r#"
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXTINF:9.009,
segment1.ts
#EXTINF:9.009,
segment2.ts
#EXT-X-ENDLIST
        "#;
        assert_eq!(
            parser.detect_playlist_type(media_content).unwrap(),
            PlaylistType::Media
        );

        // 测试未知格式
        let unknown_content = "Not a valid M3U8 file";
        assert_eq!(
            parser.detect_playlist_type(unknown_content).unwrap(),
            PlaylistType::Unknown
        );
    }

    #[test]
    fn test_parse_extinf_duration() {
        let parser = DefaultM3U8Parser::new(Box::new(MockHttpClient::new()));

        assert_eq!(parser.parse_extinf_duration("#EXTINF:10.0,").unwrap(), 10.0);
        assert_eq!(
            parser.parse_extinf_duration("#EXTINF:9.009,").unwrap(),
            9.009
        );
        assert_eq!(
            parser.parse_extinf_duration("#EXTINF:5,title").unwrap(),
            5.0
        );
    }

    #[test]
    fn test_extract_bandwidth() {
        let parser = DefaultM3U8Parser::new(Box::new(MockHttpClient::new()));

        assert_eq!(
            parser.extract_bandwidth("BANDWIDTH=1280000,RESOLUTION=854x480"),
            Some(1280000)
        );
        assert_eq!(
            parser.extract_bandwidth("RESOLUTION=854x480,BANDWIDTH=2560000"),
            Some(2560000)
        );
        assert_eq!(parser.extract_bandwidth("RESOLUTION=854x480"), None);
    }

    #[test]
    fn test_extract_resolution() {
        let parser = DefaultM3U8Parser::new(Box::new(MockHttpClient::new()));

        assert_eq!(
            parser.extract_resolution("BANDWIDTH=1280000,RESOLUTION=854x480"),
            Some((854, 480))
        );
        assert_eq!(
            parser.extract_resolution("RESOLUTION=1920x1080,BANDWIDTH=5000000"),
            Some((1920, 1080))
        );
        assert_eq!(parser.extract_resolution("BANDWIDTH=1280000"), None);
    }
}
