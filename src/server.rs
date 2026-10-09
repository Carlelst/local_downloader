//! 本地下载服务（配合油猴悬浮球脚本使用）
//!
//! `video_processor --server [--port 7777]` 常驻运行，监听 127.0.0.1。
//! 油猴脚本在 rou.video 播放页捕获伪装 playlist URL 后 POST 到本服务，
//! 服务端先做快速预检（下载 playlist 并解析分片数，约 1-2s），
//! 预检通过立即响应并放入后台队列下载，进度见服务端终端日志。
//!
//! 端点：
//! - `GET  /health`   存活探测
//! - `OPTIONS *`      CORS 预检（含 Chrome PNA 要求的 Private-Network 头）
//! - `POST /download` body: {"url": "..."} → {"ok": true, "segments": N} | 4xx {"ok": false, "error": "..."}

use anyhow::{anyhow, Context, Result};
use log::{error, info, warn};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};

use video_processor::png_stream;
use video_processor::Config;

use crate::{create_http_client, extract_png_video_id, get_cookie_for_url, get_referer_for_url};

/// 同时下载的最大视频数（每个视频内部还有分片级并发，避免多视频抢带宽打爆磁盘）
const MAX_CONCURRENT_VIDEOS: usize = 4;

#[derive(Deserialize)]
struct DownloadRequest {
    url: String,
}

/// 启动常驻 HTTP 服务
pub async fn run_server(config: Config, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {} 失败（端口被占用？）", addr))?;

    // 运行中任务去重：同一 URL 不重复下载
    let active: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));
    let video_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_VIDEOS));

    info!("下载服务已启动: http://{}", addr);
    info!(
        "等待油猴脚本任务；下载进度/失败见本终端日志。同时最多 {} 个视频，Ctrl+C 退出。",
        MAX_CONCURRENT_VIDEOS
    );

    loop {
        let (stream, peer) = listener.accept().await?;
        let config = Arc::new(config.clone());
        let active = Arc::clone(&active);
        let video_slots = Arc::clone(&video_slots);
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, config, active, video_slots).await {
                warn!("连接 {} 处理失败: {:#}", peer, e);
            }
        });
    }
}

async fn handle_conn(
    stream: TcpStream,
    config: Arc<Config>,
    active: Arc<RwLock<HashSet<String>>>,
    video_slots: Arc<Semaphore>,
) -> Result<()> {
    let mut stream = stream;
    let (method, path, body) = read_request(&mut stream).await?;

    // 所有响应统一带 CORS 头（浏览器 Private Network Access 要求回环地址响应带 PNA 头）
    let cors = "\r\nAccess-Control-Allow-Origin: *\r\n\
        Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
        Access-Control-Allow-Headers: content-type\r\n\
        Access-Control-Allow-Private-Network: true";

    match (method.as_str(), path.as_str()) {
        ("OPTIONS", _) => {
            write_response(stream, 204, "No Content", cors, "{}").await?;
        }
        ("GET", "/health") => {
            write_response(stream, 200, "OK", cors, r#"{"ok":true}"#).await?;
        }
        ("POST", "/download") => {
            let resp = handle_download(&body, &config, &active, &video_slots).await;
            let (status, text) = resp;
            let code = if status { "OK" } else { "Bad Request" };
            write_response(stream, if status { 200 } else { 400 }, code, cors, &text).await?;
        }
        _ => {
            write_response(
                stream,
                404,
                "Not Found",
                cors,
                r#"{"ok":false,"error":"not found"}"#,
            )
            .await?;
        }
    }
    Ok(())
}

/// playlist 内容分类结果
#[derive(Debug, PartialEq)]
enum PlaylistKind {
    /// rou.video 家族伪装流（分片为 .png/.jpg 伪装）→ png_stream 管线
    PngDisguised,
    /// 标准 M3U8（missav 等，分片 .ts/.mp4，可能带 AES）→ CLI m3u8 管线
    StandardM3U8,
}

fn classify_playlist(playlist: &str) -> PlaylistKind {
    const DISGUISED_EXT: [&str; 4] = [".png", ".jpg", ".jpeg", ".webp"];
    for line in playlist.lines() {
        let seg = line.trim();
        if seg.is_empty() || seg.starts_with('#') {
            continue;
        }
        // 去掉 query 后看扩展名
        let path = seg
            .split(['?', '#'])
            .next()
            .unwrap_or(seg)
            .to_ascii_lowercase();
        if DISGUISED_EXT.iter().any(|e| path.ends_with(e)) {
            return PlaylistKind::PngDisguised;
        }
        // 第一个真实分片行即可判定
        return PlaylistKind::StandardM3U8;
    }
    PlaylistKind::StandardM3U8
}

/// 处理下载请求：解析 JSON → 去重 → 预检 playlist → 按内容分派管线 → 入后台队列
/// 返回 (是否成功, JSON 响应体)
async fn handle_download(
    body: &[u8],
    config: &Config,
    active: &Arc<RwLock<HashSet<String>>>,
    video_slots: &Arc<Semaphore>,
) -> (bool, String) {
    // 解析请求体
    let req: DownloadRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return (false, json_error(&format!("请求体不是有效 JSON: {}", e))),
    };
    let url = req.url.trim().to_string();
    if url.is_empty() {
        return (false, json_error("url 为空"));
    }

    // 同 URL 去重（下载中重复点击）
    {
        let mut set = active.write().await;
        if set.contains(&url) {
            return (false, json_error("该 URL 已在下载队列中"));
        }
        set.insert(url.clone());
    }

    // 快速预检：下载 playlist 并分类（1-2s），提前暴露 401 过期/404 等错误
    let referer = get_referer_for_url(&url, config);
    let cookie = get_cookie_for_url(&url, config);
    let client = match create_http_client(&referer, &cookie, config.segment_timeout) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            active.write().await.remove(&url);
            return (false, json_error(&format!("创建 HTTP 客户端失败: {:#}", e)));
        }
    };

    let raw = match video_processor::download_with_retry(&client, &url, 120, 1, 1, None).await {
        Ok(d) => d,
        Err(e) => {
            active.write().await.remove(&url);
            return (false, json_error(&format!("playlist 下载失败: {:#}", e)));
        }
    };
    let playlist_text = match png_stream::decode_resource(&raw) {
        Ok(png_stream::PngStreamResource::Playlist(text)) => text,
        Ok(png_stream::PngStreamResource::Ts(_)) => {
            active.write().await.remove(&url);
            return (
                false,
                json_error("输入 URL 是分片而非 playlist，请填播放页 index.png/index.jpg 或 playlist.m3u8 的地址"),
            );
        }
        Err(e) => {
            active.write().await.remove(&url);
            return (
                false,
                json_error(&format!("playlist 资源解析失败: {:#}", e)),
            );
        }
    };
    let kind = classify_playlist(&playlist_text);
    let segment_count = match png_stream::parse_segment_urls(&playlist_text, &url) {
        Ok(list) => list.len(),
        Err(e) => {
            active.write().await.remove(&url);
            return (false, json_error(&format!("解析分片列表失败: {:#}", e)));
        }
    };

    // 入后台队列
    let config = Arc::new(config.clone());
    let active = Arc::clone(&active);
    let video_slots = Arc::clone(&video_slots);
    let task_url = url.clone();
    tokio::spawn(async move {
        // 最多同时 2 个视频在下载，排队等许可
        let _permit = video_slots.acquire().await.expect("信号量不应关闭");

        // 命名与 CLI 流程一致：语义段 + MD5 前 8 位防撞
        let output_name = {
            let name = extract_png_video_id(&task_url);
            let hash = format!("{:x}", md5::compute(&task_url));
            format!("{}_{}", name, &hash[..hash.len().min(8)])
        };

        info!(
            "[Server] 开始下载 ({} 分片, {:?}): {}",
            segment_count, kind, task_url
        );
        let result = match kind {
            PlaylistKind::PngDisguised => {
                png_stream::download_png_stream(&config, client, &task_url, &output_name)
                    .await
                    .map(|p| p.display().to_string())
            }
            PlaylistKind::StandardM3U8 => {
                // 与 CLI 完全同一条 m3u8 管线（断点续传/AES/CF bypass）
                let download_dir = std::path::Path::new(&config.output_directory).to_path_buf();
                let _ = tokio::fs::create_dir_all(&download_dir).await;
                let temp_dir = download_dir.join("temp");
                let _ = tokio::fs::create_dir_all(&temp_dir).await;
                let urls = vec![task_url.clone()];
                crate::process_m3u8_batch(&urls, &config, &download_dir, &temp_dir)
                    .await
                    .map(|_| format!("{}（输出文件名见上方管线日志）", config.output_directory))
            }
        };
        active.write().await.remove(&task_url);

        match result {
            Ok(path) => info!("[Server] ✓ 下载完成: {}", path),
            Err(e) => error!("[Server] ✗ 下载失败: {} — {:#}", task_url, e),
        }
        drop(_permit);
    });

    (
        true,
        format!(r#"{{"ok":true,"segments":{}}}"#, segment_count),
    )
}

/// 读一个完整 HTTP 请求（request line + headers + body）
/// 返回 (method, path, body)
async fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>)> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    if method.is_empty() || path.is_empty() {
        return Err(anyhow!("非法请求行: {:?}", request_line));
    }

    // 读 headers，找 content-length
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break; // 空行 = header 结束
        }
        if let Some(v) = trimmed
            .strip_prefix("content-length:")
            .or_else(|| trimmed.strip_prefix("Content-Length:"))
        {
            content_length = v.trim().parse().context("content-length 解析失败")?;
        }
    }

    if content_length > 1024 * 1024 {
        return Err(anyhow!("请求体过大: {} 字节", content_length));
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;
    Ok((method, path, body))
}

/// 写 HTTP 响应（JSON 专用）
async fn write_response(
    mut stream: TcpStream,
    status: u16,
    reason: &str,
    cors: &str,
    json: &str,
) -> Result<()> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json{}{}Content-Length: {}\r\n\r\n{}",
        status,
        reason,
        cors,
        "\r\n",
        json.len(),
        json
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn json_error(msg: &str) -> String {
    // 转义双引号/反斜杠，保证 JSON 合法
    let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
    format!(r#"{{"ok":false,"error":"{}"}}"#, escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_download_request() {
        let req: DownloadRequest =
            serde_json::from_str(r#"{"url":"https://gokj68.xyz/hls/a/index.png?v=1&auth=x"}"#)
                .expect("应解析成功");
        assert!(req.url.contains("auth=x"));
        assert!(serde_json::from_str::<DownloadRequest>(r#"{"nope":1}"#).is_err());
    }

    #[test]
    fn test_json_error_escapes() {
        let out = json_error("bad \"url\" \\ path");
        assert!(out.contains("bad \\\"url\\\" \\\\ path"));
        // 保证是合法 JSON
        let val: serde_json::Value = serde_json::from_str(&out).expect("应合法");
        assert_eq!(val["ok"], false);
    }

    #[test]
    fn test_classify_playlist() {
        // 伪装流：分片是 .png
        let png_pl =
            "#EXTM3U\n#EXT-X-VERSION:3\nhttps://gokj68.xyz/hls/a/CLS-000-v1-a1.png?v=6&auth=x\n";
        assert_eq!(classify_playlist(png_pl), PlaylistKind::PngDisguised);
        // 伪装流：明文 m3u8 但分片是假 .jpg（rn245 变体）
        let jpg_pl = "#EXTM3U\nhttps://v.rn245.xyz/x/C-000.jpg?v=1\n";
        assert_eq!(classify_playlist(jpg_pl), PlaylistKind::PngDisguised);
        // 标准 m3u8：.ts 分片 + AES
        let ts_pl = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\nhttps://surrit.com/x/seg-0.ts\n";
        assert_eq!(classify_playlist(ts_pl), PlaylistKind::StandardM3U8);
        // 标准 m3u8：mp4 分片（fMP4）
        let mp4_pl = "#EXTM3U\nhttps://cdn.example.com/v/seg-0.mp4\n";
        assert_eq!(classify_playlist(mp4_pl), PlaylistKind::StandardM3U8);
    }

    #[tokio::test]
    async fn test_read_request_and_response() {
        // 模拟一个完整 POST 请求
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let payload = r#"{"url":"https://x/index.png"}"#;

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let (m, p, b) = read_request(&mut sock).await.unwrap();
            assert_eq!(m, "POST");
            assert_eq!(p, "/download");
            assert_eq!(b, payload.as_bytes());
            write_response(sock, 200, "OK", "", r#"{"ok":true}"#)
                .await
                .unwrap();
        });

        let mut conn = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        let req = format!(
            "POST /download HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            payload.len(),
            payload
        );
        conn.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.ends_with(r#"{"ok":true}"#));
    }
}
