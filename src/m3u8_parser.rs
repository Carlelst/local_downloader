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
    let (urls, key, iv, _, _, _) = parse_m3u8_with_depth(m3u8_url, client, 0).await?;
    Ok((urls, key, iv))
}

/// 解析 M3U8 URL 并返回 ts 片段 URL、AES 密钥及 IV，以及片段格式。
pub async fn parse_m3u8_with_format(
    m3u8_url: &str,
    client: &Client,
) -> Result<(
    Vec<String>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    SegmentFormat,
    Option<String>,
    u64,
)> {
    parse_m3u8_with_depth(m3u8_url, client, 0).await
}

// 定义返回类型别名，简化代码
// 元素: (片段URL列表, AES密钥, AES IV [显式], 片段格式, fMP4 init段URL, MEDIA-SEQUENCE 基准值)
type ParseResult = Result<(
    Vec<String>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    SegmentFormat,
    Option<String>,
    u64,
)>;

// 内部实现函数，添加递归深度参数防止无限递归
// 手写 Box<dyn Future> 处理递归；+ Send 使 future 可跨 tokio::spawn 使用（server 复用管线需要）
fn parse_m3u8_with_depth<'a>(
    m3u8_url: &'a str,
    client: &'a Client,
    depth: usize,
) -> Pin<Box<dyn Future<Output = ParseResult> + Send + 'a>> {
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
            return Ok((
                vec![m3u8_url.to_string()],
                None,
                None,
                SegmentFormat::Ts,
                None,
                0,
            ));
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
) -> ParseResult {
    // 提取所有片段
    let mut ts_urls = Vec::new();
    let mut processed_urls = HashSet::new(); // 用于去重
    let mut segment_format = SegmentFormat::Ts; // 默认 MPEG-TS
    let mut init_url: Option<String> = None; // fMP4 init 段（EXT-X-MAP）
    let lines: Vec<&str> = m3u8_content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // EXT-X-MAP：fMP4 的 init 段（ftyp+moov），必须先于所有 moof 分片写入
        if line.starts_with("#EXT-X-MAP:") {
            if let Some(idx) = line.find("URI=\"") {
                let rest = &line[idx + 5..];
                if let Some(end) = rest.find('"') {
                    let raw = rest[..end].trim();
                    if !raw.is_empty() {
                        init_url = Some(transform_line_to_url(raw, base_url));
                        debug!("检测到 EXT-X-MAP init 段: {}", init_url.as_ref().unwrap());
                    }
                }
            }
        }

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
    // RFC 8216：未提供 IV 时，应使用 Media Sequence Number 作为 IV（16 字节大端）
    let media_sequence: u64 = m3u8_content
        .lines()
        .find_map(|l| l.trim().strip_prefix("#EXT-X-MEDIA-SEQUENCE:"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0);

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

                // RFC 8216 §5.2：无显式 IV 时返回 None，由解密方按
                // 「MEDIA-SEQUENCE + 分片序号」逐分片计算 IV（见 segment_processor）
                let iv = match captures.get(4) {
                    Some(m) => {
                        let iv_str = m.as_str();
                        match hex::decode(iv_str) {
                            Ok(iv) => {
                                if iv.len() != 16 {
                                    warn!("IV 长度异常: {} 字节，期望 16 字节", iv.len());
                                }
                                Some(iv)
                            }
                            Err(e) => {
                                return Err(anyhow!(
                                    "EXT-X-KEY IV 无效 ({})，原始值: {}",
                                    e,
                                    iv_str
                                ));
                            }
                        }
                    }
                    None => None,
                };

                // 下载密钥
                info!("从 URL 获取 AES 密钥: {}", key_url);
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

                            info!(
                                "成功获取 AES 密钥，长度: {} 字节, 前 4 字节: {:02x?}",
                                key_bytes.len(),
                                &key_bytes[..key_bytes.len().min(4)]
                            );
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

    Ok((
        ts_urls,
        aes_key,
        iv,
        segment_format,
        init_url,
        media_sequence,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 极简本地 HTTP 测试服务器：
    /// 对每个 GET 请求按"路径包含"匹配返回预置响应，直到路由表清空
    fn spawn_test_server(
        listener: TcpListener,
        routes: Vec<(String, u16, Vec<u8>)>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut routes = routes;
            while !routes.is_empty() {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let request = String::from_utf8_lossy(&buf);
                let Some(idx) = routes
                    .iter()
                    .position(|(p, _, _)| request.contains(p.as_str()))
                else {
                    continue;
                };
                let (_, status, body) = routes.remove(idx);
                let reason = if status == 200 { "OK" } else { "Not Found" };
                let head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    reason,
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        })
    }

    fn bind_test_server() -> (u16, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定测试端口失败");
        let port = listener.local_addr().unwrap().port();
        (port, listener)
    }

    fn no_proxy_client() -> Client {
        Client::builder()
            .no_proxy()
            .build()
            .expect("构建测试客户端失败")
    }

    /// RFC 8216: 无 IV 时应使用 Media Sequence Number 作为 IV（16 字节大端）
    #[tokio::test]
    async fn test_aes128_without_iv_uses_media_sequence() {
        let (port, listener) = bind_test_server();
        let server = spawn_test_server(
            listener,
            vec![
                ("/key.bin".to_string(), 200, vec![0xf3; 16]),
                (
                    "/index.m3u8".to_string(),
                    200,
                    format!(
                        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-TARGETDURATION:9\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://127.0.0.1:{port}/key.bin\"\n#EXTINF:8.333,\nhttp://127.0.0.1:{port}/seg0.ts\n"
                    )
                    .into_bytes(),
                ),
            ],
        );

        let m3u8_url = format!("http://127.0.0.1:{port}/index.m3u8");
        let (ts_urls, aes_key, aes_iv, _, init_url, media_sequence) =
            parse_m3u8_with_format(&m3u8_url, &no_proxy_client())
                .await
                .expect("解析 m3u8 应成功");

        server.join().expect("测试服务器线程 panic");

        assert_eq!(ts_urls.len(), 1);
        assert!(
            init_url.is_none(),
            "无 EXT-X-MAP 的播放列表 init 段应为 None"
        );
        assert_eq!(aes_key, Some(vec![0xf3; 16]));
        // RFC 8216 §5.2：无显式 IV 时解析器返回 None + MEDIA-SEQUENCE 基准值，
        // 由解密方按「MEDIA-SEQUENCE + 分片序号」逐分片计算 16 字节 IV
        assert!(aes_iv.is_none(), "无显式 IV 时应返回 None 而非伪造 IV");
        assert_eq!(media_sequence, 7, "应解析出 MEDIA-SEQUENCE 基准值");
    }

    /// 显式 IV 时应直接使用提供的 IV
    #[tokio::test]
    async fn test_aes128_with_explicit_iv() {
        let (port, listener) = bind_test_server();
        let server = spawn_test_server(
            listener,
            vec![
                ("/key.bin".to_string(), 200, vec![0xab; 16]),
                (
                    "/index.m3u8".to_string(),
                    200,
                    format!(
                        "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://127.0.0.1:{port}/key.bin\",IV=0x42b7b97ba0e9d9fd9b6ee632b3b19ec8\n#EXTINF:8.333,\nhttp://127.0.0.1:{port}/seg0.ts\n"
                    )
                    .into_bytes(),
                ),
            ],
        );

        let m3u8_url = format!("http://127.0.0.1:{port}/index.m3u8");
        let (_, aes_key, aes_iv, _, _, _) = parse_m3u8_with_format(&m3u8_url, &no_proxy_client())
            .await
            .expect("解析 m3u8 应成功");

        server.join().expect("测试服务器线程 panic");

        assert_eq!(aes_key, Some(vec![0xab; 16]));
        assert_eq!(
            aes_iv,
            Some(hex::decode("42b7b97ba0e9d9fd9b6ee632b3b19ec8").unwrap())
        );
    }

    /// fMP4 播放列表应解析出 EXT-X-MAP 指定的 init 段 URL（相对路径需拼接）
    #[tokio::test]
    async fn test_fmp4_ext_x_map_parsed() {
        let (port, listener) = bind_test_server();
        let server = spawn_test_server(
            listener,
            vec![(
                "/index.m3u8".to_string(),
                200,
                format!(
                    "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-MAP:URI=\"init-v1-a1.mp4\"\n#EXTINF:4.000,\nseg-1-v1-a1.m4s\n#EXTINF:4.000,\nseg-2-v1-a1.m4s\n"
                )
                .into_bytes(),
            )],
        );

        let m3u8_url = format!("http://127.0.0.1:{port}/index.m3u8");
        let (ts_urls, _, _, segment_format, init_url, _) =
            parse_m3u8_with_format(&m3u8_url, &no_proxy_client())
                .await
                .expect("解析 m3u8 应成功");

        server.join().expect("测试服务器线程 panic");

        assert_eq!(ts_urls.len(), 2);
        assert_eq!(
            segment_format,
            SegmentFormat::Fmp4,
            ".m4s 分片应识别为 fMP4"
        );
        // 相对路径 init 段应基于 m3u8 所在目录拼接
        assert_eq!(
            init_url,
            Some(format!("http://127.0.0.1:{port}/init-v1-a1.mp4")),
            "EXT-X-MAP 的相对 URI 应解析为完整 URL"
        );
    }
}
