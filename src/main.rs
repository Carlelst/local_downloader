use anyhow::{anyhow, Result};
use chrono::Local;
use env_logger::Builder;
use log::{debug, error, info, trace, warn};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::{sleep, timeout, Duration, Instant};

use video_processor::{
    decrypt_segment,
    ffmpeg::start_ffmpeg,
    m3u8_parser::parse_m3u8_with_format,
    parse_log_level,
    segment_processor::{media_sequence_iv, process_segment},
};

/// 保存断点续传元数据（已下载分片索引列表），供正常完成与中途失败两条路径复用
async fn save_checkpoint(downloaded: &RwLock<HashSet<usize>>, metadata_path: &Path) {
    let list: Vec<usize> = downloaded.read().await.iter().copied().collect();
    if let Ok(json) = serde_json::to_string(&list) {
        if let Ok(mut file) = File::create(metadata_path).await {
            let _ = file.write_all(json.as_bytes()).await;
            debug!("已保存断点续传数据: {} 个片段", list.len());
        }
    }
}

// 从 M3U8 URL 提取视频 ID 用于输出命名
// 优先级：
//   1. URL 路径中的 UUID（surrit.com 风格：/UUID/.../video.m3u8）
//   2. URL 路径中最后一个非通用/非分辨率段
//   3. URL 的 MD5 哈希（前 10 位）
fn extract_video_id(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let segments: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();

        // 1. 寻找 UUID 模式（8-4-4-4-12 十六进制）
        for seg in &segments {
            if is_uuid(seg) {
                return seg.to_string();
            }
        }

        // 2. 从后往前找第一个有意义的段（跳过分辨率/通用名）
        const SKIP_NAMES: &[&str] = &[
            "video", "index", "playlist", "master", "chunk", "seg", "segment", "stream", "play",
            "media", "hls",
        ];
        for seg in segments.iter().rev() {
            let lower = seg.to_ascii_lowercase();
            if lower.ends_with(".m3u8") || SKIP_NAMES.iter().any(|n| lower == *n) {
                continue;
            }
            if is_resolution(&lower) {
                continue;
            }
            return sanitize_filename(seg);
        }
    }
    // 3. 回退到 MD5
    let url_hash = format!("{:x}", md5::compute(url));
    format!("cf_{}", &url_hash[..10])
}

// 判断字符串是否是分辨率/码率标记（720p / 1080p / 4k / 1920x1080 / 2000kb / 3000kbps）
fn is_resolution(s: &str) -> bool {
    // 码率模式：数字 + kb/mb/kbps/mbps
    for suffix in ["kbps", "mbps", "kb", "mb"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            return !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit());
        }
    }
    if let Some(stripped) = s.strip_suffix('p') {
        return !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit());
    }
    if let Some(stripped) = s.strip_suffix('k') {
        return !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit());
    }
    if s.contains('x') {
        let parts: Vec<&str> = s.split('x').collect();
        if parts.len() == 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return true;
        }
    }
    false
}

// 判断字符串是否是 UUID 格式
fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && s.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

// 清理文件名（移除不安全字符）
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_resolution_bitrate_segments() {
        // 码率段（vostrely.com 风格 /2000kb/hls/）
        assert!(is_resolution("2000kb"));
        assert!(is_resolution("3000kb"));
        assert!(is_resolution("2000kbps"));
        assert!(is_resolution("800mbps"));
        // 非纯数字前缀不是码率
        assert!(!is_resolution("hZ08wQf1kb"));
    }

    #[test]
    fn test_is_resolution_video_resolutions() {
        assert!(is_resolution("720p"));
        assert!(is_resolution("1080p"));
        assert!(is_resolution("4k"));
        assert!(is_resolution("1920x1080"));
        assert!(!is_resolution("720x"));
        assert!(!is_resolution("p720"));
    }

    #[test]
    fn test_is_resolution_normal_names() {
        assert!(!is_resolution("hZ08wQf1"));
        assert!(!is_resolution("index"));
        assert!(!is_resolution("hls"));
        assert!(!is_resolution(""));
    }

    #[test]
    fn test_extract_video_id_vostrely() {
        // 码率段 2000kb 应被跳过，命中 hash 段
        let id = extract_video_id("https://vostrely.com/20260516/hZ08wQf1/2000kb/hls/index.m3u8");
        assert_eq!(id, "hZ08wQf1");
    }

    #[test]
    fn test_extract_video_id_surrit_uuid() {
        let id = extract_video_id(
            "https://surrit.com/2e997f15-8b5e-4a83-9210-3d3e4dfe09a3/720p/video.m3u8",
        );
        assert_eq!(id, "2e997f15-8b5e-4a83-9210-3d3e4dfe09a3");
    }

    #[test]
    fn test_extract_video_id_phncdn_style() {
        // index-v1-a1.m3u8 被跳过；720P_4000K_414878711.mp4 非纯分辨率标记，
        // 从后往前第一个命中
        let id = extract_video_id(
            "https://ev-h.phncdn.com/hls/videos/202209/02/414878711/720P_4000K_414878711.mp4/index-v1-a1.m3u8",
        );
        assert_eq!(id, "720P_4000K_414878711.mp4");
    }

    #[test]
    fn test_extract_video_id_md5_fallback() {
        // 无有效段时回退 MD5 前缀
        let id = extract_video_id("https://example.com/master.m3u8");
        assert!(id.starts_with("cf_"));
        assert_eq!(id.len(), 13);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    let mut config = video_processor::load_config()?;

    // 初始化日志
    Builder::new()
        .filter(None, parse_log_level(&config.log_level))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();

    info!("加载配置: {:?}", config);
    info!("使用并发下载数: {}", config.max_concurrent_downloads);

    // 增加自适应并发控制
    let adaptive_concurrency = Arc::new(RwLock::new(config.max_concurrent_downloads));

    // 动态调整缓冲区大小
    let buffer_size = if config.buffer_size == 0 {
        4 * 1024 * 1024 // 默认4MB
    } else if config.buffer_size < 1024 * 1024 {
        1024 * 1024 // 最小1MB
    } else {
        config.buffer_size
    };

    info!("使用缓冲区大小: {} 字节", buffer_size);

    // 检查是否需要清除缓存
    if std::env::args().any(|arg| arg == "--clear-cache") {
        info!("清除所有下载缓存...");
        let download_dir = Path::new(&config.output_directory);
        let temp_dir = download_dir.join("temp");
        if temp_dir.exists() {
            match tokio::fs::remove_dir_all(&temp_dir).await {
                Ok(_) => info!("缓存目录已清除"),
                Err(e) => warn!("清除缓存失败: {}", e),
            }
            match tokio::fs::create_dir_all(&temp_dir).await {
                Ok(_) => info!("已创建新的缓存目录"),
                Err(e) => warn!("创建缓存目录失败: {}", e),
            }
        }
    }

    // 检查是否指定了比特率
    let mut global_referer = config.referer.clone();
    for arg in std::env::args() {
        if arg.starts_with("--bitrate=") {
            let bitrate = arg.split('=').nth(1).unwrap_or("2M").to_string();
            info!("使用命令行指定的视频比特率: {}", bitrate);
            config.video_bitrate = Some(bitrate);
        } else if arg == "--copy" {
            // 强制使用copy模式
            info!("使用copy模式，不重新编码视频");
            config.video_codec = "copy".to_string();
            config.use_hw_accel = false;
        } else if arg == "--no-hw" {
            // 禁用硬件加速
            info!("禁用硬件加速");
            config.use_hw_accel = false;
        } else if arg.starts_with("--referer=") {
            global_referer = arg.split('=').nth(1).unwrap_or(&global_referer).to_string();
            info!("使用命令行指定的全局 Referer: {}", global_referer);
        }
    }

    // 根据 URL 域名获取对应的 Referer
    fn get_referer_for_url(url: &str, config: &video_processor::Config) -> String {
        // 尝试从 URL 中提取域名
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                // 先尝试完整匹配域名
                if let Some(referer) = config.referers.get(host) {
                    return referer.clone();
                }
                // 尝试匹配顶级域名（如 surrit.com）
                let parts: Vec<&str> = host.split('.').collect();
                if parts.len() >= 2 {
                    let domain = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
                    if let Some(referer) = config.referers.get(&domain) {
                        return referer.clone();
                    }
                }
            }
        }
        // 回退到全局默认 referer
        config.referer.clone()
    }

    // 根据 URL 获取对应的 Cookie 字符串
    fn get_cookie_for_url(url: &str, config: &video_processor::Config) -> String {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                // 先尝试完整匹配域名
                if let Some(cookie) = config.cookies.get(host) {
                    return cookie.clone();
                }
                // 尝试匹配顶级域名
                let parts: Vec<&str> = host.split('.').collect();
                if parts.len() >= 2 {
                    let domain = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
                    if let Some(cookie) = config.cookies.get(&domain) {
                        return cookie.clone();
                    }
                }
            }
        }
        String::new()
    }

    // 创建 HTTP client 的辅助函数
    fn create_http_client(referer: &str, cookie: &str, timeout_secs: u64) -> Result<Client> {
        // 读取系统代理设置（macOS 系统偏好设置 -> 网络 -> 代理）
        // reqwest 默认会读取 HTTP_PROXY/HTTPS_PROXY 环境变量，
        // 但在 macOS 上系统代理不会自动写入环境变量，需要手动处理
        let mut builder = Client::builder()
            // 总超时只作兜底（慢速代理下大分片需要较长时间），
            // 真正的停滞检测由 stream_download 的逐块超时负责
            .timeout(Duration::from_secs((timeout_secs * 10).max(120)))
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(10)
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36");

        // 检查系统代理环境变量
        let proxy_url = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
            .or_else(|_| std::env::var("ALL_PROXY"))
            .or_else(|_| std::env::var("all_proxy"))
            .ok();

        // 如果环境变量没有设置，尝试读取 macOS 系统代理
        let proxy_url = if proxy_url.is_none() {
            #[cfg(target_os = "macos")]
            {
                // 从 scutil 读取系统代理（scutil 输出格式： "<Key> : <Value>"）
                if let Ok(output) = std::process::Command::new("scutil").arg("--proxy").output() {
                    let s = String::from_utf8_lossy(&output.stdout);
                    // 精确按 key 解析，避免 "HTTPProxy" 前缀误匹配 "HTTPSProxy"
                    let mut http_proxy: Option<String> = None;
                    let mut http_port: Option<String> = None;
                    let mut http_enabled = false;
                    let mut https_enabled = false;

                    fn split_kv(line: &str) -> Option<(&str, &str)> {
                        // 接受 " : " 或 ":" 分隔
                        let line = line.trim();
                        if let Some(idx) = line.find(" : ") {
                            Some((line[..idx].trim(), line[idx + 3..].trim()))
                        } else if let Some(idx) = line.find(':') {
                            Some((line[..idx].trim(), line[idx + 1..].trim()))
                        } else {
                            None
                        }
                    }

                    for line in s.lines() {
                        let Some((key, value)) = split_kv(line) else {
                            continue;
                        };
                        match key {
                            "HTTPProxy" => {
                                if !value.is_empty() {
                                    http_proxy = Some(value.to_string());
                                }
                            }
                            "HTTPPort" => {
                                if !value.is_empty() {
                                    http_port = Some(value.to_string());
                                }
                            }
                            "HTTPEnable" => http_enabled = value == "1",
                            "HTTPSEnable" => https_enabled = value == "1",
                            _ => {}
                        }
                    }

                    // 如果 HTTPS 代理开启，使用它；否则尝试 HTTP 代理
                    let use_proxy = https_enabled || http_enabled;
                    if use_proxy {
                        if let (Some(host), Some(port)) = (http_proxy, http_port) {
                            debug!("检测到系统代理: {}:{}", host, port);
                            Some(format!("http://{}:{}", host, port))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        } else {
            proxy_url
        };

        if let Some(url) = proxy_url {
            debug!("使用代理: {}", url);
            match reqwest::Proxy::all(&url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                }
                Err(e) => {
                    warn!("代理配置失败: {}，继续不使用代理", e);
                }
            }
        }

        builder
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert("Accept", "*/*".parse().unwrap());
                headers.insert(
                    "Accept-Language",
                    "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap(),
                );
                // 注意：不要手动设置 Accept-Encoding，让 reqwest 自动处理 gzip/deflate/br 解压
                headers.insert("Connection", "keep-alive".parse().unwrap());
                // 注意：不要发送 Sec-Fetch-* 头！
                // eporner 等 CDN 会将 Sec-Fetch-Site: cross-site 视为盗链，
                // 302 重定向到 na.mp4 导致 404；不发送这些头则畅通
                headers.insert("sec-ch-ua-platform", "macOS".parse().unwrap());
                headers.insert(
                    "sec-ch-ua",
                    "\"Google Chrome\";v=\"149\", \"Chromium\";v=\"149\", \"Not)A;Brand\";v=\"24\""
                        .parse()
                        .unwrap(),
                );
                headers.insert("sec-ch-ua-mobile", "?0".parse().unwrap());
                headers.insert("Referer", referer.parse().unwrap());
                // 如果有自定义 Cookie（用于绕过 Cloudflare），添加 Cookie 头
                if !cookie.is_empty() {
                    if let Ok(cookie_value) = cookie.parse() {
                        headers.insert("Cookie", cookie_value);
                    }
                }
                headers
            })
            .build()
            .map_err(|e| anyhow!("创建HTTP客户端失败: {}", e))
    }

    // 确保输出目录存在
    let download_dir = Path::new(&config.output_directory);
    if !download_dir.exists() {
        create_dir_all(download_dir).await?;
    }
    let temp_dir = download_dir.join("temp");
    if !temp_dir.exists() {
        create_dir_all(&temp_dir).await?;
    }

    for (i, m3u8_url) in config.m3u8_urls.iter().enumerate() {
        if m3u8_url.trim().is_empty() {
            continue;
        }

        // Cloudflare 旁路：检测到 surrit.com / fourhoi.com 等走 Python+curl-cffi
        if video_processor::cloudflare_bypass::CloudflareBypassDownloader::needs_bypass(m3u8_url) {
            let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let downloader =
                video_processor::cloudflare_bypass::CloudflareBypassDownloader::new(&project_root);
            let url_referer = get_referer_for_url(m3u8_url, &config);
            let url_cookie = get_cookie_for_url(m3u8_url, &config);
            // 智能命名：优先用 URL 中的 UUID，回退到 MD5
            let base_filename = extract_video_id(m3u8_url);
            let output_path = download_dir.join(format!("{}.mp4", base_filename));
            match downloader
                .download(m3u8_url, &url_referer, &url_cookie, &output_path)
                .await
            {
                Ok(_) => {
                    info!(
                        "[CF Bypass] ✓ M3U8 [{}] 处理完成: {}",
                        i + 1,
                        output_path.display()
                    );
                    continue;
                }
                Err(e) => {
                    error!("[CF Bypass] ✗ M3U8 [{}] 失败: {}", i + 1, e);
                    continue;
                }
            }
        }

        debug!(
            "处理 M3U8 文件 [{}/{}]: {}",
            i + 1,
            config.m3u8_urls.len(),
            m3u8_url
        );

        // 根据当前 URL 的域名获取对应的 Referer 和 Cookie，并创建专用的 client
        let url_referer = get_referer_for_url(m3u8_url, &config);
        let url_cookie = get_cookie_for_url(m3u8_url, &config);
        info!("URL {} 使用 Referer: {}", m3u8_url, url_referer);
        if !url_cookie.is_empty() {
            debug!("URL {} 使用自定义 Cookie", m3u8_url);
        }
        let client = Arc::new(create_http_client(
            &url_referer,
            &url_cookie,
            config.segment_timeout,
        )?);

        // 计算一个基于URL的唯一文件名（智能提取 UUID/有意义段，回退 MD5）
        // 语义名后附加 URL 短哈希：不同 URL 可能提取出相同语义段（如 …/a/2000kb/ 与 …/a/3000kb/ 都得到 "a"），
        // 不加哈希会导致断点缓存与输出文件互相污染
        let base_filename = {
            let name = extract_video_id(m3u8_url);
            let hash = format!("{:x}", md5::compute(m3u8_url));
            format!("{}_{}", name, &hash[..hash.len().min(8)])
        };
        let metadata_path = temp_dir.join(format!("{}.json", base_filename));
        let segments_dir = temp_dir.join(&base_filename);
        if !segments_dir.exists() {
            create_dir_all(&segments_dir).await?;
        }

        // 解析M3U8获取片段链接和加密信息
        debug!("解析M3U8: {}", m3u8_url);
        let (ts_urls, aes_key, aes_iv, segment_format, init_url, media_sequence) =
            match parse_m3u8_with_format(m3u8_url, &client).await {
                Ok(result) => result,
                Err(e) => {
                    // 单个 URL 失败（如 410 签名过期/无效）不中断整体任务，继续下一个
                    error!("M3U8 [{}] 解析失败，跳过: {}", i + 1, e);
                    continue;
                }
            };
        let total_segments = ts_urls.len();

        if total_segments == 0 {
            error!("未找到视频片段，跳过处理");
            continue;
        }

        // 验证实际可访问的片段数量
        if config.validate_segments && total_segments > 0 {
            info!("正在验证片段是否可访问...");
            let test_segment = &ts_urls[0];
            match client.head(test_segment).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        warn!("片段验证失败，状态码: {}", resp.status());
                    } else {
                        info!("片段验证成功");
                    }
                }
                Err(e) => {
                    warn!("片段验证失败: {}", e);
                }
            }
        }

        info!("找到 {} 个视频片段，开始下载", total_segments);

        // 清除之前缓存的片段元数据（如果使用--force选项）
        if std::env::args().any(|arg| arg == "--force") {
            info!("强制重新下载所有片段");
            if metadata_path.exists() {
                match tokio::fs::remove_file(&metadata_path).await {
                    Ok(_) => info!("已清除之前的下载记录"),
                    Err(e) => warn!("清除下载记录失败: {}", e),
                }
            }
        }

        // 加载之前的下载进度（如果存在）
        let mut downloaded_segments = HashSet::new();
        if metadata_path.exists() {
            match File::open(&metadata_path).await {
                Ok(mut file) => {
                    let mut json_content = String::new();
                    if file.read_to_string(&mut json_content).await.is_ok() {
                        if let Ok(segments) = serde_json::from_str::<Vec<usize>>(&json_content) {
                            for idx in segments {
                                if idx < total_segments {
                                    downloaded_segments.insert(idx);
                                }
                            }
                            info!(
                                "发现断点续传数据，已下载 {}/{} 个片段",
                                downloaded_segments.len(),
                                total_segments
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!("读取断点续传数据失败: {}", e);
                }
            }
        }

        // 准备下载新片段
        let segments = Arc::new(RwLock::new(HashMap::<usize, Vec<u8>>::with_capacity(
            total_segments,
        )));
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads));
        // 只创建需要处理的片段数量的通道容量
        let pending_segments = total_segments - downloaded_segments.len();
        let (notify_tx, mut notify_rx) = mpsc::channel(pending_segments.max(1));
        let downloaded = Arc::new(RwLock::new(downloaded_segments.clone()));
        let to_download = Arc::new(RwLock::new(HashSet::new()));
        // 重试耗尽仍失败的分片：跳过写入缺口、计入 incomplete 判定、让进度监控退出
        let failed_segments = Arc::new(RwLock::new(HashSet::new()));

        // 启动段下载任务
        let mut download_tasks = Vec::new();

        // 预先填充已下载片段到segments数据结构中，但不重新通知处理
        for idx in 0..total_segments {
            let is_downloaded = downloaded_segments.contains(&idx);

            if is_downloaded {
                // 加载已下载片段
                let segment_path = segments_dir.join(format!("{}.ts", idx));
                match File::open(&segment_path).await {
                    Ok(mut file) => {
                        let mut data = Vec::new();
                        if let Ok(_) = file.read_to_end(&mut data).await {
                            let mut segments_lock = segments.write().await;
                            segments_lock.insert(idx, data);
                            debug!("从缓存加载片段 {}", idx);
                            // 注意：不发送通知，只预加载
                        }
                    }
                    Err(_) => {
                        // 文件不存在或无法读取，需要重新下载
                        warn!("缓存片段 {} 读取失败，重新下载", idx);
                        to_download.write().await.insert(idx);
                    }
                }
            } else {
                to_download.write().await.insert(idx);
            }
        }

        // 只为需要下载的片段创建任务
        let to_download_count = to_download.read().await.len();
        let _downloads_per_batch = if to_download_count > 50 {
            // 分批下载，避免一次创建过多任务
            info!("分批下载 {} 个片段", to_download_count);
            std::cmp::min(
                30,                          // 每批最多30个
                (to_download_count + 2) / 3, // 大约分3批
            )
        } else {
            to_download_count
        };

        let download_status = Arc::new(RwLock::new(HashMap::<usize, (usize, Instant)>::new())); // 记录每个片段的下载状态和字节数
        let bandwidth_monitor = Arc::new(RwLock::new(Vec::<f64>::with_capacity(10))); // 存储最近10次带宽测量

        for idx in to_download.read().await.iter().copied().collect::<Vec<_>>() {
            let url = ts_urls[idx].clone();
            let client = Arc::clone(&client);
            let segments = Arc::clone(&segments);
            let notify_tx = notify_tx.clone();
            let aes_key = aes_key.clone();
            let aes_iv = aes_iv.clone();
            let timeout = config.segment_timeout;
            let retries = config.download_retries;
            let retry_interval = config.retry_interval;
            let semaphore = Arc::clone(&semaphore);
            let segments_dir = segments_dir.clone();
            let downloaded = Arc::clone(&downloaded);
            let to_download = Arc::clone(&to_download);
            let download_status_clone = Arc::clone(&download_status);
            let adaptive_concurrency_clone = Arc::clone(&adaptive_concurrency);
            let bandwidth_monitor_clone = Arc::clone(&bandwidth_monitor);
            let failed_segments_clone = Arc::clone(&failed_segments);

            // 启动下载任务
            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("获取信号量失败");
                let start_time = Instant::now();

                // 记录开始下载
                {
                    let mut status = download_status_clone.write().await;
                    status.insert(idx, (0, start_time));
                }

                // 数据校验失败（损坏/污染）时需要重新下载，最多尝试 3 次
                let mut last_err = None;
                for attempt in 0..3 {
                    if attempt > 0 {
                        warn!("片段 {} 进行第 {} 次重新下载...", idx, attempt + 1);
                        sleep(Duration::from_secs(2)).await;
                    }
                    match process_segment(
                        client.clone(),
                        url.clone(),
                        idx,
                        aes_key.clone(),
                        aes_iv.clone(),
                        segments.clone(),
                        notify_tx.clone(),
                        timeout,
                        retries,
                        retry_interval,
                        media_sequence,
                    )
                    .await
                    {
                        Ok(data) => {
                            // 成功下载片段后保存到磁盘
                            let segment_path = segments_dir.join(format!("{}.ts", idx));
                            if let Ok(mut file) = File::create(&segment_path).await {
                                if let Err(e) = file.write_all(&data).await {
                                    warn!("保存片段 {} 失败: {}", idx, e);
                                } else {
                                    let mut downloaded_lock = downloaded.write().await;
                                    downloaded_lock.insert(idx);

                                    // 计算下载速度
                                    let elapsed = start_time.elapsed();
                                    let bytes = data.len();
                                    let mbps = if elapsed.as_millis() > 0 {
                                        (bytes as f64 * 8.0) / (elapsed.as_millis() as f64) * 1000.0
                                            / (1024.0 * 1024.0)
                                    } else {
                                        0.0
                                    };

                                    // 记录带宽
                                    {
                                        let mut bw = bandwidth_monitor_clone.write().await;
                                        bw.push(mbps);
                                        if bw.len() > 10 {
                                            bw.remove(0);
                                        }

                                        // 动态调整并发数
                                        if bw.len() >= 3 {
                                            let avg_mbps: f64 =
                                                bw.iter().sum::<f64>() / bw.len() as f64;
                                            let mut concurrency =
                                                adaptive_concurrency_clone.write().await;

                                            if avg_mbps < 1.0 && *concurrency > 4 {
                                                // 带宽低，减少并发
                                                *concurrency = (*concurrency * 3) / 4;
                                                debug!(
                                                    "带宽低 ({:.2} Mbps)，减少并发至 {}",
                                                    avg_mbps, concurrency
                                                );
                                            } else if avg_mbps > 5.0
                                                && *concurrency
                                                    < config.max_concurrent_downloads * 2
                                            {
                                                // 带宽充足，增加并发
                                                *concurrency = (*concurrency * 5) / 4;
                                                debug!(
                                                    "带宽充足 ({:.2} Mbps)，增加并发至 {}",
                                                    avg_mbps, concurrency
                                                );
                                            }
                                        }
                                    }

                                    debug!(
                                        "成功下载并保存片段 {} ({} 字节, {:.2} Mbps)",
                                        idx, bytes, mbps
                                    );
                                }
                            }
                            to_download.write().await.remove(&idx);
                            last_err = None;
                            break;
                        }
                        Err(e) => {
                            let msg = format!("{:#}", e);
                            error!("片段 {} 处理失败 (第 {} 次): {}", idx, attempt + 1, msg);
                            // 内层 download_with_retry 已对网络类错误做过完整重试，
                            // 外层重试仅针对数据损坏类失败（校验/解密/转换），
                            // 避免持久性网络错误造成 3×(1+N) 次请求的乘法放大
                            let is_data_error = msg.contains("校验失败")
                                || msg.contains("解密")
                                || msg.contains("转换失败");
                            last_err = Some(anyhow!(msg));
                            if !is_data_error {
                                break;
                            }
                        }
                    }
                }

                if let Some(e) = last_err {
                    error!(
                        "片段 {} 多次尝试后仍失败，将在输出中缺失（文件将标记为 incomplete）: {}",
                        idx, e
                    );
                    failed_segments_clone.write().await.insert(idx);
                    to_download.write().await.remove(&idx);
                }

                // 清理状态
                {
                    let mut status = download_status_clone.write().await;
                    status.remove(&idx);
                }
            });

            download_tasks.push(task);
        }
        drop(notify_tx); // 关闭发送端

        // 如果没有要下载的片段则直接进入FFmpeg处理阶段
        let progress = Arc::new(RwLock::new((0, pending_segments)));
        let progress_update_time = Arc::new(RwLock::new(Instant::now()));

        if to_download.read().await.is_empty() {
            info!("所有片段已经下载完成，直接进入处理阶段");
        } else {
            // 等待一段时间后检查下载进度
            let progress_check_interval = Duration::from_secs(5);
            let max_stuck_time = Duration::from_secs(30); // 30秒无进度则增加重试

            let progress_clone = Arc::clone(&progress);
            let progress_time_clone = Arc::clone(&progress_update_time);
            let to_download_clone = Arc::clone(&to_download);
            tokio::spawn(async move {
                loop {
                    sleep(progress_check_interval).await;

                    let current_progress = {
                        let progress_lock = progress_clone.read().await;
                        progress_lock.clone()
                    };

                    let last_update = {
                        let time_lock = progress_time_clone.read().await;
                        *time_lock
                    };

                    let stuck_time = Instant::now().duration_since(last_update);
                    if stuck_time > max_stuck_time {
                        // 如果长时间没有进度，重新检查哪些片段仍需下载
                        let to_download_count = to_download_clone.read().await.len();
                        warn!(
                            "下载进度停滞 ({:?})，仍有 {} 个片段未下载",
                            stuck_time, to_download_count
                        );

                        if to_download_count == 0 {
                            // 所有片段都已开始下载，但可能卡在某些片段
                            break;
                        }
                    }

                    if current_progress.0 >= current_progress.1
                        || (current_progress.1 > 0 && current_progress.0 == current_progress.1)
                    {
                        // 所有需要下载的片段都已处理，退出监控
                        break;
                    }
                }
            });
        }

        // 获取FFmpeg进程
        let (mut ffmpeg, output_file) = start_ffmpeg(&config, segment_format).await?;
        let ffmpeg_stdin = ffmpeg
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("无法打开 FFmpeg 标准输入"))?;
        let mut buffered_stdin = BufWriter::with_capacity(buffer_size, ffmpeg_stdin);

        // fMP4 必须先写入 EXT-X-MAP 指定的 init 段（ftyp+moov），
        // 否则 moof 分片缺少解码所需的编码参数，FFmpeg 会直接退出。
        // init 段处理失败只跳过当前 URL，不中止批量任务（与解析失败的处理一致）
        if let Some(init_url) = &init_url {
            info!("下载 fMP4 init 段: {}", init_url);
            let init_result: Result<Vec<u8>> = async {
                let resp = match timeout(Duration::from_secs(30), client.get(init_url).send()).await
                {
                    Ok(Ok(r)) if r.status().is_success() => r,
                    Ok(Ok(r)) => return Err(anyhow!("init 段下载失败: HTTP {}", r.status())),
                    Ok(Err(e)) => return Err(anyhow!("init 段请求失败: {}", e)),
                    Err(_) => return Err(anyhow!("下载 init 段超时")),
                };
                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| anyhow!("读取 init 段数据失败: {}", e))?;
                let mut data = bytes.to_vec();
                // RFC 8216 §4.3.2.5：EXT-X-KEY 同样作用于 init section，加密流需解密
                if let Some(key) = &aes_key {
                    let iv = aes_iv
                        .clone()
                        .unwrap_or_else(|| media_sequence_iv(media_sequence));
                    data = decrypt_segment(&data, key, &iv)
                        .map_err(|e| anyhow!("init 段解密失败: {}", e))?;
                }
                Ok(data)
            }
            .await;

            match init_result {
                Ok(init_data) => {
                    info!("init 段处理完成: {} 字节", init_data.len());
                    if let Err(e) = buffered_stdin.write_all(&init_data).await {
                        error!("M3U8 [{}] 写入 init 段失败，跳过该 URL: {}", i + 1, e);
                        drop(buffered_stdin);
                        let _ = ffmpeg.start_kill();
                        for task in download_tasks.drain(..) {
                            task.abort();
                        }
                        save_checkpoint(&downloaded, &metadata_path).await;
                        continue;
                    }
                }
                Err(e) => {
                    error!("M3U8 [{}] init 段处理失败，跳过该 URL: {}", i + 1, e);
                    drop(buffered_stdin);
                    let _ = ffmpeg.start_kill();
                    for task in download_tasks.drain(..) {
                        task.abort();
                    }
                    save_checkpoint(&downloaded, &metadata_path).await;
                    continue;
                }
            }
        }

        // 进度追踪并写入FFmpeg
        let mut processed_segments = 0;
        let mut next_idx = 0;
        let mut processed_indices = HashSet::new();
        let mut missing_segments = HashSet::new();

        // 有两部分工作：
        // 1. 将已下载的片段按顺序写入FFmpeg
        // 2. 等待新下载的片段通知并写入FFmpeg

        // 先尝试按顺序写入已下载的片段
        let mut segments_lock = segments.write().await;
        while let Some(data) = segments_lock.remove(&next_idx) {
            // 检查数据大小，太小的数据可能是无效的
            if data.len() < 100 {
                warn!("跳过太小的片段 {} ({} 字节)", next_idx, data.len());
                missing_segments.insert(next_idx);
            } else {
                debug!("写入已缓存的片段 {} 到 FFmpeg", next_idx);
                buffered_stdin.write_all(&data).await?;
                processed_indices.insert(next_idx);
                processed_segments += 1;
            }
            next_idx += 1;
        }
        drop(segments_lock);

        // 如果有新下载的片段通知，继续处理
        let expected_segments = to_download.read().await.len();
        let mut received_segments = 0;

        while let Some(idx) = notify_rx.recv().await {
            received_segments += 1;

            // 更新进度
            {
                let mut progress_lock = progress.write().await;
                *progress_lock = (received_segments, expected_segments);

                let mut time_lock = progress_update_time.write().await;
                *time_lock = Instant::now();
            }

            if !processed_indices.contains(&idx) {
                processed_indices.insert(idx);
                processed_segments += 1;

                if processed_segments % 10 == 0 || processed_segments == total_segments {
                    info!(
                        "进度: {}/{} ({:.1}%)",
                        processed_segments,
                        total_segments,
                        (processed_segments as f64 / total_segments as f64) * 100.0
                    );
                }
            }

            // 按顺序写入FFmpeg
            let mut segments_lock = segments.write().await;

            // 尝试处理所有连续的片段
            loop {
                match segments_lock.remove(&next_idx) {
                    Some(data) => {
                        // 检查数据大小，太小的数据可能是无效的
                        if data.len() < 100 {
                            warn!("跳过太小的片段 {} ({} 字节)", next_idx, data.len());
                            missing_segments.insert(next_idx);
                        } else {
                            trace!("写入片段 {} 到 FFmpeg", next_idx);
                            buffered_stdin.write_all(&data).await?;
                        }
                        next_idx += 1;
                    }
                    None => {
                        // 分片重试耗尽：跳过缺口继续写入，避免视频在缺口处永久截断
                        if failed_segments.read().await.contains(&next_idx) {
                            warn!("跳过失败片段 {}（输出将标记为 incomplete）", next_idx);
                            missing_segments.insert(next_idx);
                            next_idx += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // 保存当前已下载片段的信息，用于断点续传
        save_checkpoint(&downloaded, &metadata_path).await;

        // 确保所有数据都被写入
        buffered_stdin.flush().await?;
        drop(buffered_stdin);

        // 检查是否有缺失的片段（已经在前面收集）
        let missing_segments_list: Vec<usize> = missing_segments.into_iter().collect();

        // 等待所有下载任务完成
        for task in download_tasks {
            let _ = task.await;
        }

        // 重试耗尽的失败分片同样视为缺失
        let failed_count = failed_segments.read().await.len();

        // 等待FFmpeg处理完成
        if ffmpeg.wait().await?.success() {
            info!("FFmpeg 处理成功: {}", output_file);

            // 如果处理成功但有片段缺失，重命名文件以标明不完整
            if !missing_segments_list.is_empty() || failed_count > 0 {
                let output_path = Path::new(&output_file);
                let file_stem = output_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let extension = output_path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy();
                let incomplete_filename = format!("{}_incomplete.{}", file_stem, extension);
                let incomplete_path = output_path.with_file_name(&incomplete_filename);

                if let Err(e) = tokio::fs::rename(output_path, &incomplete_path).await {
                    error!("重命名不完整文件失败: {}", e);
                } else {
                    warn!(
                        "由于缺少片段，已将文件重命名为: {}",
                        incomplete_path.display()
                    );
                }
            }
        } else {
            error!("FFmpeg 处理失败: {}", output_file);
        }
    }

    info!("总耗时: {:.2?}", start_time.elapsed());
    Ok(())
}
