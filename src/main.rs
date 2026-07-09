use anyhow::{anyhow, Result};
use chrono::Local;
use env_logger::Builder;
use log::{debug, error, info, warn};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::{sleep, Duration, Instant};

use video_processor::{
    ffmpeg::start_ffmpeg,
    m3u8_parser::{parse_m3u8_with_format, SegmentFormat},
    parse_log_level,
    segment_processor::process_segment,
};

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

    // 从 M3U8 URL 提取视频 ID 用于输出命名
    // 优先级：
    //   1. URL 路径中的 UUID（surrit.com 风格：/UUID/.../video.m3u8）
    //   2. URL 路径中最后一个非 m3u8 段
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

            // 2. 找倒数第二个段（路径里通常 <id>/<resolution>/<file>）
            //    排除掉 "video.m3u8" 之类的通用名
            if segments.len() >= 2 {
                let second_last = segments[segments.len() - 2];
                if !second_last.is_empty()
                    && second_last != "video"
                    && !second_last.ends_with(".m3u8")
                {
                    return sanitize_filename(second_last);
                }
            }
        }
        // 3. 回退到 MD5
        let url_hash = format!("{:x}", md5::compute(url));
        format!("cf_{}", &url_hash[..10])
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
            .timeout(Duration::from_secs(timeout_secs))
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
                // 从 scutil 读取系统代理
                if let Ok(output) = std::process::Command::new("scutil").arg("--proxy").output() {
                    let s = String::from_utf8_lossy(&output.stdout);
                    let mut http_proxy: Option<String> = None;
                    let mut http_port: Option<String> = None;
                    let mut http_enabled = false;
                    let mut https_enabled = false;

                    for line in s.lines() {
                        let line = line.trim();
                        // 解析类似 "HTTPProxy : 127.0.0.1" 或 "HTTPProxy:127.0.0.1"
                        if line.starts_with("HTTPProxy") {
                            if let Some(pos) = line.find(':') {
                                let value = line[pos + 1..].trim().to_string();
                                if !value.is_empty() {
                                    http_proxy = Some(value);
                                }
                            }
                        } else if line.starts_with("HTTPPort") {
                            if let Some(pos) = line.find(':') {
                                let value = line[pos + 1..].trim().to_string();
                                if !value.is_empty() {
                                    http_port = Some(value);
                                }
                            }
                        } else if line.starts_with("HTTPEnable") {
                            http_enabled = line.contains(": 1") || line.ends_with(": 1");
                        } else if line.starts_with("HTTPSEnable") {
                            https_enabled = line.contains(": 1") || line.ends_with(": 1");
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
                headers.insert("Sec-Fetch-Dest", "empty".parse().unwrap());
                headers.insert("Sec-Fetch-Mode", "cors".parse().unwrap());
                headers.insert("Sec-Fetch-Site", "cross-site".parse().unwrap());
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
            // 智能命名：优先用 URL 中的 UUID，回退到 MD5
            let base_filename = extract_video_id(m3u8_url);
            let output_path = download_dir.join(format!("{}.mp4", base_filename));
            match downloader
                .download(m3u8_url, &url_referer, &output_path)
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

        // 计算一个基于URL的唯一文件名
        let url_hash = format!("{:x}", md5::compute(m3u8_url));
        let base_filename = url_hash[..10].to_string();
        let metadata_path = temp_dir.join(format!("{}.json", base_filename));
        let segments_dir = temp_dir.join(&base_filename);
        if !segments_dir.exists() {
            create_dir_all(&segments_dir).await?;
        }

        // 解析M3U8获取片段链接和加密信息
        debug!("解析M3U8: {}", m3u8_url);
        let (ts_urls, aes_key, aes_iv, segment_format) =
            parse_m3u8_with_format(m3u8_url, &client).await?;
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

            // 启动下载任务
            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("获取信号量失败");
                let start_time = Instant::now();

                // 记录开始下载
                {
                    let mut status = download_status_clone.write().await;
                    status.insert(idx, (0, start_time));
                }

                match process_segment(
                    client,
                    url.clone(),
                    idx,
                    aes_key,
                    aes_iv,
                    segments,
                    notify_tx.clone(),
                    timeout,
                    retries,
                    retry_interval,
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
                                                avg_mbps, *concurrency
                                            );
                                        } else if avg_mbps > 5.0
                                            && *concurrency < config.max_concurrent_downloads * 2
                                        {
                                            // 带宽充足，增加并发
                                            *concurrency = (*concurrency * 5) / 4;
                                            debug!(
                                                "带宽充足 ({:.2} Mbps)，增加并发至 {}",
                                                avg_mbps, *concurrency
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
                    }
                    Err(e) => {
                        error!("片段 {} 处理失败: {:?}", idx, e);
                    }
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
            while let Some(data) = segments_lock.remove(&next_idx) {
                // 检查数据大小，太小的数据可能是无效的
                if data.len() < 100 {
                    warn!("跳过太小的片段 {} ({} 字节)", next_idx, data.len());
                    missing_segments.insert(next_idx);
                } else {
                    debug!("写入片段 {} 到 FFmpeg", next_idx);
                    buffered_stdin.write_all(&data).await?;
                }
                next_idx += 1;
            }
        }

        // 保存当前已下载片段的信息，用于断点续传
        let downloaded_list = {
            let downloaded_lock = downloaded.read().await;
            downloaded_lock.iter().copied().collect::<Vec<_>>()
        };
        let json_data = serde_json::to_string(&downloaded_list)?;
        if let Ok(mut file) = File::create(&metadata_path).await {
            let _ = file.write_all(json_data.as_bytes()).await;
            debug!("已保存断点续传数据: {} 个片段", downloaded_list.len());
        }

        // 确保所有数据都被写入
        buffered_stdin.flush().await?;
        drop(buffered_stdin);

        // 检查是否有缺失的片段（已经在前面收集）
        let missing_segments_list: Vec<usize> = missing_segments.into_iter().collect();

        // 等待所有下载任务完成
        for task in download_tasks {
            let _ = task.await;
        }

        // 等待FFmpeg处理完成
        if ffmpeg.wait().await?.success() {
            info!("FFmpeg 处理成功: {}", output_file);

            // 如果处理成功但有片段缺失，重命名文件以标明不完整
            if !missing_segments_list.is_empty() {
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
