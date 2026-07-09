use anyhow::{anyhow, Context, Result};
use log::{debug, error, info, warn};
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

/// 片段格式枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentFormat {
    /// MPEG-TS 格式（.ts 文件）
    Ts,
    /// Fragmented MP4 格式（.m4s 文件）
    Fmp4,
}

impl Default for SegmentFormat {
    fn default() -> Self {
        SegmentFormat::Ts
    }
}

/// 解析 M3U8 URL 并返回 ts 片段 URL、AES 密钥及 IV。
///
/// # 参数
/// - `m3u8_url`: M3U8 文件的 URL
/// - `client`: HTTP 客户端
///
/// # 返回
/// 返回包含 ts 片段 URL 列表、可选的 AES 密钥和 IV。
pub async fn parse_m3u8(
    m3u8_url: &str,
    client: &Client,
) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let (urls, key, iv, _) = parse_m3u8_with_depth(m3u8_url, client, 0).await?;
    Ok((urls, key, iv))
}

/// 解析 M3U8 URL 并返回 ts 片段 URL、AES 密钥及 IV，以及片段格式。
pub async fn parse_m3u8_with_format(
    m3u8_url: &str,
    client: &Client,
) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>, SegmentFormat)> {
    parse_m3u8_with_depth(m3u8_url, client, 0).await
}

// 定义返回类型别名，简化代码
type ParseResult = Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>, SegmentFormat)>;

// 内部实现函数，添加递归深度参数防止无限递归
fn parse_m3u8_with_depth<'a>(
    m3u8_url: &'a str,
    client: &'a Client,
    depth: usize,
) -> Pin<Box<dyn Future<Output = ParseResult> + 'a>> {
    Box::pin(async move {
        // 防止过深递归
        if depth > 5 {
            return Err(anyhow!("M3U8 递归层级过深 (>5)，可能存在循环引用"));
        }

        if m3u8_url.trim().is_empty() {
            return Err(anyhow!("M3U8 URL 不能为空"));
        }

        info!("获取 M3U8 内容，URL: {}", m3u8_url);

        // 设置超时时间为10秒
        let response = match timeout(Duration::from_secs(10), client.get(m3u8_url).send()).await {
            Ok(res) => res.context("无法获取 M3U8 内容")?,
            Err(_) => return Err(anyhow!("获取 M3U8 内容超时")),
        };

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP 错误: {}, URL: {}", status, m3u8_url));
        }

        let m3u8_content = response.text().await.context("读取 M3U8 内容失败")?;

        if m3u8_content.trim().is_empty() {
            return Err(anyhow!("M3U8 内容为空，URL: {}", m3u8_url));
        }

        // 检查是否是 jpeg 文件
        if m3u8_url.ends_with(".jpeg") || m3u8_url.ends_with(".jpg") {
            info!("检测到 jpeg 文件，直接返回 URL");
            return Ok((vec![m3u8_url.to_string()], None, None, SegmentFormat::Ts));
        }

        if !m3u8_content.contains("#EXTM3U") {
            warn!("可能不是有效的 M3U8 文件: 未找到 #EXTM3U 标识符");
        }

        info!("M3U8 内容长度: {} 字节", m3u8_content.len());
        debug!("M3U8 内容: {}", m3u8_content);

        // 解析基础 URL，用于构建相对路径
        let base_url = match Url::parse(m3u8_url) {
            Ok(url) => {
                // 获取 m3u8 文件所在目录的 URL
                let mut path = url.path().to_string();
                if let Some(pos) = path.rfind('/') {
                    path.truncate(pos + 1);
                }
                let mut base = url.clone();
                base.set_path(&path);
                base
            }
            Err(e) => {
                warn!("URL 解析失败: {}", e);
                return Err(anyhow!("无效的 M3U8 URL: {}", m3u8_url));
            }
        };

        debug!("M3U8 基础 URL: {}", base_url);

        // 先检查是否是主播放列表（包含其他 m3u8）
        // 正则表达式模式匹配EXT-X-STREAM-INF行和下一行的m3u8链接
        let master_playlist_re = Regex::new(r#"#EXT-X-STREAM-INF:.*?\n(.*?\.m3u8.*?)\n"#)?;
        if let Some(captures) = master_playlist_re.captures(&m3u8_content) {
            if let Some(sub_m3u8_path) = captures.get(1) {
                let sub_path = sub_m3u8_path.as_str().trim();
                let sub_url = if sub_path.starts_with("http") {
                    sub_path.to_string()
                } else {
                    base_url.join(sub_path)?.to_string()
                };

                info!("检测到主播放列表，递归解析子 M3U8: {}", sub_url);

                // 通过递增depth参数防止无限递归
                return parse_m3u8_with_depth(&sub_url, client, depth + 1).await;
            }
        }

        // 如果不是主播放列表或没有找到子M3U8，则处理为媒体播放列表
        process_media_playlist(&m3u8_content, &base_url, client).await
    })
}

// 处理媒体播放列表（包含具体的TS片段）
async fn process_media_playlist(
    m3u8_content: &str,
    base_url: &Url,
    client: &Client,
) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>, SegmentFormat)> {
    // 提取所有片段
    let mut ts_urls = Vec::new();
    let mut processed_urls = HashSet::new(); // 用于去重
    let mut segment_format = SegmentFormat::Ts; // 默认 MPEG-TS
    let lines: Vec<&str> = m3u8_content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // 如果是EXTINF行，下一行通常是片段URL
        if line.starts_with("#EXTINF:") && i + 1 < lines.len() {
            let next_line = lines[i + 1].trim();
            if !next_line.starts_with('#') && !next_line.is_empty() {
                let ts_url = transform_line_to_url(next_line, base_url);
                if !ts_url.is_empty() && !processed_urls.contains(&ts_url) {
                    // 检测片段格式
                    if ts_url.contains(".m4s") || ts_url.contains("SegmentTemplate") {
                        segment_format = SegmentFormat::Fmp4;
                    }
                    processed_urls.insert(ts_url.clone());
                    ts_urls.push(ts_url);
                }
            }
        } else if !line.starts_with('#') && !line.is_empty() {
            // 直接是片段URL的情况
            let ts_url = transform_line_to_url(line, base_url);
            if !ts_url.is_empty() && !processed_urls.contains(&ts_url) {
                // 检测片段格式
                if ts_url.contains(".m4s") || ts_url.contains("SegmentTemplate") {
                    segment_format = SegmentFormat::Fmp4;
                }
                processed_urls.insert(ts_url.clone());
                ts_urls.push(ts_url);
            }
        }
    }

    // 清理无效 URLs
    ts_urls.retain(|url| !url.trim().is_empty());

    // 仅保留唯一的URL
    ts_urls.dedup();

    if ts_urls.is_empty() {
        warn!("未从 M3U8 内容中找到任何 TS 片段链接");
    } else {
        debug!("找到 {} 个唯一片段: {:?}", ts_urls.len(), ts_urls);
    }

    // 解析加密信息
    let key_re = Regex::new(
        r#"#EXT-X-KEY:METHOD=([A-Z0-9-]+)(?:,URI=(?:"([^"]+)"|'([^']+)'))?(?:,IV=(?:0x)?([0-9a-fA-F]+))?"#,
    )?;

    let (aes_key, iv) = if let Some(captures) = key_re.captures(m3u8_content) {
        let method = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        match method {
            "AES-128" => {
                let uri_group = captures.get(2).or_else(|| captures.get(3));
                let key_url = match uri_group.map(|m| {
                    let url_str = m.as_str();
                    if url_str.starts_with("http") {
                        url_str.to_string()
                    } else {
                        base_url
                            .join(url_str)
                            .map(|u| u.to_string())
                            .unwrap_or_else(|_| {
                                warn!("无法解析密钥 URL，使用原始字符串: {}", url_str);
                                url_str.to_string()
                            })
                    }
                }) {
                    Some(url) => url,
                    None => return Err(anyhow!("METHOD=AES-128 但未提供密钥 URI")),
                };

                let iv = captures
                    .get(4)
                    .map(|m| {
                        let iv_str = m.as_str();
                        match hex::decode(iv_str) {
                            Ok(iv) => {
                                if iv.len() != 16 {
                                    warn!("IV 长度异常: {} 字节，期望 16 字节", iv.len());
                                }
                                iv
                            }
                            Err(e) => {
                                warn!("IV 解码失败: {}, 原始值: {}", e, iv_str);
                                vec![0; 16] // 使用全零IV作为回退
                            }
                        }
                    })
                    .or_else(|| {
                        info!("未提供 IV，使用默认全零 IV");
                        Some(vec![0; 16])
                    });

                // 下载密钥
                debug!("从 URL 获取 AES 密钥: {}", key_url);
                let aes_key = match timeout(Duration::from_secs(5), client.get(&key_url).send())
                    .await
                {
                    Ok(result) => match result {
                        Ok(key_response) => {
                            if !key_response.status().is_success() {
                                error!("密钥请求失败，HTTP状态码: {}", key_response.status());
                                return Err(anyhow!(
                                    "获取密钥失败，HTTP状态码: {}",
                                    key_response.status()
                                ));
                            }

                            let key_bytes = key_response
                                .bytes()
                                .await
                                .context("读取密钥内容失败")?
                                .to_vec();

                            if key_bytes.len() != 16 {
                                warn!("AES 密钥长度异常: {} 字节，期望 16 字节", key_bytes.len());
                            }

                            info!("成功获取 AES 密钥，长度: {} 字节", key_bytes.len());
                            Some(key_bytes)
                        }
                        Err(e) => {
                            error!("获取密钥失败: {}", e);
                            return Err(anyhow!("获取密钥失败: {}", e));
                        }
                    },
                    Err(_) => {
                        error!("获取密钥超时: {}", key_url);
                        return Err(anyhow!("获取密钥超时"));
                    }
                };

                (aes_key, iv)
            }
            "NONE" => {
                debug!("检测到未加密的流（METHOD=NONE）");
                (None, None)
            }
            _ => {
                warn!("未知的加密方法: {}", method);
                (None, None)
            }
        }
    } else {
        info!("未找到加密信息，假定未加密");
        (None, None)
    };

    info!(
        "解析结果: {} 个 TS 片段，加密: {}，格式: {}",
        ts_urls.len(),
        if aes_key.is_some() { "是" } else { "否" },
        if segment_format == SegmentFormat::Ts {
            "MPEG-TS"
        } else {
            "fMP4"
        }
    );

    Ok((ts_urls, aes_key, iv, segment_format))
}

/// 转换相对路径为完整 URL
///
/// # 参数
/// - `line`: M3U8 文件中的行
/// - `base_url`: M3U8 文件的基础 URL
///
/// # 返回
/// 返回完整的 URL。
fn transform_line_to_url(line: &str, base_url: &Url) -> String {
    let line = line.trim();

    if line.is_empty() {
        return String::new();
    }

    // 如果已经是完整 URL
    if line.starts_with("http://") || line.starts_with("https://") {
        return line.to_string();
    }

    // 尝试使用基础URL拼接
    match base_url.join(line) {
        Ok(url) => url.to_string(),
        Err(e) => {
            warn!("URL 拼接失败: {}，使用原始字符串: {}", e, line);
            line.to_string()
        }
    }
}

/// 兼容性包装器 - 使用新的M3U8解析器实现
/// 保持与现有代码的兼容性
pub async fn parse_m3u8_new(
    m3u8_url: &str,
    client: &Client,
) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    use crate::domain::services::M3U8Parser;
    use crate::infrastructure::{DefaultHttpClient, DefaultM3U8Parser};

    // 创建新的HTTP客户端包装器
    let http_client = Box::new(DefaultHttpClient::with_client(client.clone()));

    // 创建新的M3U8解析器
    let parser = DefaultM3U8Parser::new(http_client);

    // 解析播放列表
    let playlist = parser.parse(m3u8_url).await?;

    // 提取片段URL列表
    let ts_urls: Vec<String> = playlist
        .segments
        .iter()
        .map(|segment| segment.url.clone())
        .collect();

    // 提取加密信息
    let (aes_key, aes_iv) = if let Some(encryption) = playlist.encryption {
        (encryption.key_data, encryption.iv)
    } else {
        (None, None)
    };

    Ok((ts_urls, aes_key, aes_iv))
}
