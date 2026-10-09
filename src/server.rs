//! 本地下载服务（配合油猴悬浮球脚本使用）
//!
//! `video_processor --server [--port 7777]` 常驻运行，监听 127.0.0.1。
//! 油猴脚本在 rou.video 播放页捕获伪装 playlist URL 后 POST 到本服务，
//! 服务端先做快速预检（下载 playlist 并解析分片数，约 1-2s），
//! 预检通过立即响应并放入后台队列下载，进度见服务端终端日志。
//!
//! 端点：
//! - `GET  /health`   存活探测
//! - `GET  /status`   任务列表 JSON（面板轮询显示进度）：{"ok":true,"counts":{...},"tasks":[...]}
//! - `OPTIONS *`      CORS 预检（含 Chrome PNA 要求的 Private-Network 头）
//! - `POST /download` body: {"url": "..."} → {"ok": true, "segments": N} | 4xx {"ok": false, "error": "..."}
//! - `POST /cancel`   body: {"url": "..."} 取消排队/下载中任务（下载中任务在分片检查点快速退出）
//! - `POST /forget`   body: {"url": "..."} 从任务列表移除已结束（done/failed/cancelled）任务
//!
//! 取消语义：queued → 直接出队；downloading → 置取消标志，管线在分片检查点快速失败，
//! 任务落 cancelled 终态并清理分段缓存（temp）与半成品输出；失败任务保留缓存以支持续传。

use anyhow::{anyhow, Context, Result};
use log::{error, info, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};

use video_processor::png_stream;
use video_processor::png_stream::TaskSignal;
use video_processor::Config;

use crate::{
    create_http_client, extract_png_video_id, extract_video_id, get_cookie_for_url,
    get_referer_for_url,
};

/// 同时下载的最大视频数（每个视频内部还有分片级并发，避免多视频抢带宽打爆磁盘）
const MAX_CONCURRENT_VIDEOS: usize = 4;

#[derive(Deserialize)]
struct DownloadRequest {
    url: String,
}

/// /cancel 与 /forget 共用的请求体
#[derive(Deserialize)]
struct UrlRequest {
    url: String,
}

/// 单个下载任务的服务端状态（面板通过 GET /status 轮询）
#[derive(Clone, serde::Serialize)]
struct TaskState {
    url: String,
    /// 输出文件名（不含扩展名）
    name: String,
    /// queued | downloading | cancelling | done | failed | cancelled
    state: String,
    segments: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    started_at: u64,
    finished_at: u64,
}

#[derive(Clone)]
struct TaskEntry {
    state: TaskState,
    /// 取消标志 + 分片完成计数（与下载管线共享）
    signal: Arc<TaskSignal>,
}

type TaskMap = Arc<RwLock<HashMap<String, TaskEntry>>>;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 活动状态：不允许重复入队，也不能直接从列表移除
fn is_active_state(state: &str) -> bool {
    matches!(state, "queued" | "downloading" | "cancelling")
}

/// 视频稳定键：host + 路径（去掉 query 与 index/playlist 文件名）。
/// 同一视频的 auth/exp 轮换只改 query，重复入队/轮询匹配都按此键归一
fn video_key(url: &str) -> String {
    const PLAYLIST_FILES: [&str; 5] = [
        "index.png",
        "index.jpg",
        "index.jpeg",
        "index.webp",
        "playlist.m3u8",
    ];
    match url::Url::parse(url) {
        Ok(u) => {
            let path = u.path();
            let path = match path.rsplit_once('/') {
                Some((dir, file))
                    if PLAYLIST_FILES.contains(&file.to_ascii_lowercase().as_str()) =>
                {
                    dir.to_string()
                }
                _ => path.to_string(),
            };
            format!("{}{}", u.host_str().unwrap_or(""), path)
        }
        Err(_) => url.split(['?', '#']).next().unwrap_or(url).to_string(),
    }
}

/// 启动常驻 HTTP 服务
pub async fn run_server(config: Config, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {} 失败（端口被占用？）", addr))?;

    // 任务状态表（key = 下载 URL），同时承担运行中去重职责
    let tasks: TaskMap = Arc::new(RwLock::new(HashMap::new()));
    let video_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_VIDEOS));

    // 启动时清理孤儿分段缓存：对应输出 mp4 已存在的任务，缓存已无保留价值
    cleanup_orphan_temps(&config).await;

    info!("下载服务已启动: http://{}", addr);
    info!(
        "等待油猴脚本任务；任务列表见 GET /status。同时最多 {} 个视频，Ctrl+C 退出。",
        MAX_CONCURRENT_VIDEOS
    );

    loop {
        let (stream, peer) = listener.accept().await?;
        let config = Arc::new(config.clone());
        let tasks = Arc::clone(&tasks);
        let video_slots = Arc::clone(&video_slots);
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, config, tasks, video_slots).await {
                warn!("连接 {} 处理失败: {:#}", peer, e);
            }
        });
    }
}

async fn handle_conn(
    stream: TcpStream,
    config: Arc<Config>,
    tasks: TaskMap,
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
        ("GET", "/status") => {
            let text = handle_status(&tasks).await;
            write_response(stream, 200, "OK", cors, &text).await?;
        }
        ("POST", "/download") => {
            let resp = handle_download(&body, &config, &tasks, &video_slots).await;
            let (status, text) = resp;
            let code = if status { "OK" } else { "Bad Request" };
            write_response(stream, if status { 200 } else { 400 }, code, cors, &text).await?;
        }
        ("POST", "/cancel") | ("POST", "/forget") => {
            let (ok, text) = if path == "/cancel" {
                handle_cancel(&body, &tasks).await
            } else {
                handle_forget(&body, &tasks).await
            };
            let code = if ok { "OK" } else { "Bad Request" };
            write_response(stream, if ok { 200 } else { 400 }, code, cors, &text).await?;
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
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// 处理下载请求：解析 JSON → 任务表去重 → 预检 playlist → 按内容分派管线 → 入后台队列
/// 返回 (是否成功, JSON 响应体)
async fn handle_download(
    body: &[u8],
    config: &Config,
    tasks: &TaskMap,
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

    // 任务表按视频稳定键去重（auth/exp 轮换不改身份）：排队/下载中/取消中不重复入队；
    // done/failed/cancelled 再次提交 = 重新下载（刷新为最新 auth 地址）
    let signal = TaskSignal::new();
    let key = video_key(&url);
    let output_name = {
        let name = extract_png_video_id(&url);
        let hash = format!("{:x}", md5::compute(&url));
        format!("{}_{}", name, &hash[..hash.len().min(8)])
    };
    {
        let mut map = tasks.write().await;
        if let Some(existing) = map.get(&key) {
            if is_active_state(&existing.state.state) {
                return (false, json_error("该视频已在下载队列中"));
            }
        }
        map.insert(
            key.clone(),
            TaskEntry {
                state: TaskState {
                    url: url.clone(),
                    name: output_name.clone(),
                    state: "queued".to_string(),
                    segments: 0,
                    error: String::new(),
                    started_at: now_ms(),
                    finished_at: 0,
                },
                signal: Arc::clone(&signal),
            },
        );
    }

    // 快速预检：下载 playlist 并分类（1-2s），提前暴露 401 过期/404 等错误。
    // 预检失败 = 任务从未真正入队，直接从任务表移除（错误信息由响应带给面板）
    macro_rules! precheck_fail {
        ($msg:expr) => {{
            tasks.write().await.remove(&key);
            return (false, json_error(&$msg));
        }};
    }

    let referer = get_referer_for_url(&url, config);
    let cookie = get_cookie_for_url(&url, config);
    let client = match create_http_client(&referer, &cookie, config.segment_timeout) {
        Ok(c) => Arc::new(c),
        Err(e) => precheck_fail!(format!("创建 HTTP 客户端失败: {:#}", e)),
    };

    let raw = match video_processor::download_with_retry(&client, &url, 120, 1, 1, None).await {
        Ok(d) => d,
        Err(e) => precheck_fail!(format!("playlist 下载失败: {:#}", e)),
    };
    let playlist_text = match png_stream::decode_resource(&raw) {
        Ok(png_stream::PngStreamResource::Playlist(text)) => text,
        Ok(png_stream::PngStreamResource::Ts(_)) => precheck_fail!(
            "输入 URL 是分片而非 playlist，请填播放页 index.png/index.jpg 或 playlist.m3u8 的地址"
                .to_string()
        ),
        Err(e) => precheck_fail!(format!("playlist 资源解析失败: {:#}", e)),
    };
    let kind = classify_playlist(&playlist_text);
    let segment_count = match png_stream::parse_segment_urls(&playlist_text, &url) {
        Ok(list) => list.len(),
        Err(e) => precheck_fail!(format!("解析分片列表失败: {:#}", e)),
    };

    // 记录分片数（面板展示用）
    if let Some(e) = tasks.write().await.get_mut(&key) {
        e.state.segments = segment_count;
    }

    // 入后台队列
    let config = Arc::new(config.clone());
    let tasks = Arc::clone(tasks);
    let video_slots = Arc::clone(video_slots);
    let task_url = url.clone();
    let task_key = key.clone();
    tokio::spawn(async move {
        // 等待空闲许可；排队期间被取消则直接出队
        let _permit = tokio::select! {
            p = video_slots.acquire() => p.expect("信号量不应关闭"),
            _ = wait_cancelled(&signal.cancel) => {
                finalize_task(&tasks, &task_key, "cancelled", "").await;
                info!("[Server] 排队中任务已取消出队: {}", task_url);
                return;
            }
        };

        // 标记下载中
        if let Some(e) = tasks.write().await.get_mut(&task_key) {
            e.state.state = "downloading".to_string();
        }

        info!(
            "[Server] 开始下载 ({} 分片, {:?}): {}",
            segment_count, kind, task_url
        );
        let result = match kind {
            PlaylistKind::PngDisguised => png_stream::download_png_stream(
                &config,
                client,
                &task_url,
                &output_name,
                Some(Arc::clone(&signal)),
            )
            .await
            .map(|p| p.display().to_string()),
            PlaylistKind::StandardM3U8 => {
                // 与 CLI 完全同一条 m3u8 管线（断点续传/AES/CF bypass）
                let download_dir = std::path::Path::new(&config.output_directory).to_path_buf();
                let _ = tokio::fs::create_dir_all(&download_dir).await;
                let temp_dir = download_dir.join("temp");
                let _ = tokio::fs::create_dir_all(&temp_dir).await;
                let urls = vec![task_url.clone()];
                crate::process_m3u8_batch(
                    &urls,
                    &config,
                    &download_dir,
                    &temp_dir,
                    Some(Arc::clone(&signal)),
                )
                .await
                .map(|_| format!("{}（输出文件名见上方管线日志）", config.output_directory))
            }
        };

        if signal.is_cancelled() {
            match result {
                // 取消请求到达时恰好已全部完成 → 按完成处理，不清理
                Ok(path) => {
                    info!("[Server] 取消前已完成: {}", path);
                    finalize_task(&tasks, &task_key, "done", "").await;
                }
                Err(_) => {
                    // 后置清理：删除该任务的分段缓存与半成品输出
                    cleanup_task_files(&config, kind, &task_url, &output_name).await;
                    finalize_task(&tasks, &task_key, "cancelled", "").await;
                    info!("[Server] 任务已取消并清理: {}", task_url);
                }
            }
        } else {
            match result {
                Ok(path) => {
                    info!("[Server] ✓ 下载完成: {}", path);
                    finalize_task(&tasks, &task_key, "done", "").await;
                }
                Err(e) => {
                    error!("[Server] ✗ 下载失败: {} — {:#}", task_url, e);
                    // 失败保留分段缓存（断点续传价值），仅落终态
                    finalize_task(&tasks, &task_key, "failed", &format!("{:#}", e)).await;
                }
            }
        }
        drop(_permit);
    });

    (
        true,
        format!(r#"{{"ok":true,"segments":{}}}"#, segment_count),
    )
}

/// 等待取消标志置位（排队阶段的可打断等待）
async fn wait_cancelled(flag: &AtomicBool) {
    loop {
        if flag.load(Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// 落任务终态。仅当任务仍处于活动状态时写入（防止覆盖重新入队的新任务）
async fn finalize_task(tasks: &TaskMap, url: &str, state: &str, error: &str) {
    let mut map = tasks.write().await;
    if let Some(e) = map.get_mut(url) {
        if is_active_state(&e.state.state) {
            e.state.state = state.to_string();
            e.state.error = error.to_string();
            e.state.finished_at = now_ms();
        }
    }
}

/// 取消后的后置清理：删除该任务的分段缓存（断点文件）与半成品输出
async fn cleanup_task_files(config: &Config, kind: PlaylistKind, url: &str, output_name: &str) {
    let download_dir = std::path::Path::new(&config.output_directory);
    match kind {
        // PNG 伪装流分段全在内存，只有 FFmpeg 半成品输出需要清理
        PlaylistKind::PngDisguised => {
            let out = download_dir.join(format!("{}.mp4", output_name));
            if tokio::fs::remove_file(&out).await.is_ok() {
                info!("[Server] 已清理半成品输出: {}", out.display());
            }
        }
        PlaylistKind::StandardM3U8 => {
            // 命名与 process_m3u8_batch 内部一致：extract_video_id + MD5 前 8 位
            let base = {
                let name = extract_video_id(url);
                let hash = format!("{:x}", md5::compute(url));
                format!("{}_{}", name, &hash[..hash.len().min(8)])
            };
            let temp_dir = download_dir.join("temp");
            let _ = tokio::fs::remove_dir_all(temp_dir.join(&base)).await;
            let _ = tokio::fs::remove_file(temp_dir.join(format!("{}.json", base))).await;
            let _ = tokio::fs::remove_file(download_dir.join(format!("{}.mp4", base))).await;
            info!("[Server] 已清理分段缓存与半成品输出: {}", base);
        }
    }
}

/// 启动时清理孤儿分段缓存：对应输出 mp4 已存在的任务，缓存已无保留价值。
/// 崩溃残留且无输出的断点缓存保留（重启后可续传）
async fn cleanup_orphan_temps(config: &Config) {
    let download_dir = std::path::Path::new(&config.output_directory);
    let temp_dir = download_dir.join("temp");
    let mut entries = match tokio::fs::read_dir(&temp_dir).await {
        Ok(e) => e,
        Err(_) => return,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let base = name.strip_suffix(".json").unwrap_or(&name).to_string();
        if base.is_empty() {
            continue;
        }
        if download_dir.join(format!("{}.mp4", base)).exists() {
            // 目录/文件各删一次，另一边不存在时报错忽略
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
            let _ = tokio::fs::remove_file(entry.path()).await;
            info!("[Server] 清理孤儿分段缓存: {}", name);
        }
    }
}

/// GET /status：任务列表 + 各状态计数（面板轮询）
async fn handle_status(tasks: &TaskMap) -> String {
    let map = tasks.read().await;
    let mut list: Vec<&TaskEntry> = map.values().collect();
    list.sort_by_key(|e| std::cmp::Reverse(e.state.started_at));
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut arr = Vec::with_capacity(list.len());
    for e in &list {
        *counts.entry(e.state.state.clone()).or_insert(0) += 1;
        if let Ok(mut v) = serde_json::to_value(&e.state) {
            // 注入实时分片进度（done 计数在 signal 里，不落 state 快照）
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "segments_done".to_string(),
                    serde_json::json!(e.signal.done.load(Ordering::Relaxed)),
                );
            }
            arr.push(v);
        }
    }
    serde_json::json!({ "ok": true, "counts": counts, "tasks": arr }).to_string()
}

/// POST /cancel：取消排队/下载中任务
async fn handle_cancel(body: &[u8], tasks: &TaskMap) -> (bool, String) {
    let req: UrlRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return (false, json_error(&format!("请求体不是有效 JSON: {}", e))),
    };
    let key = video_key(req.url.trim());
    let mut map = tasks.write().await;
    match map.get_mut(&key) {
        Some(e) if is_active_state(&e.state.state) => {
            e.signal.cancel.store(true, Ordering::Relaxed);
            e.state.state = "cancelling".to_string();
            (true, r#"{"ok":true}"#.to_string())
        }
        Some(e) => (
            false,
            json_error(&format!("任务已结束（{}），无需取消", e.state.state)),
        ),
        None => (false, json_error("任务不存在（可能服务已重启）")),
    }
}

/// POST /forget：从任务列表移除已结束任务（活动任务请先取消）
async fn handle_forget(body: &[u8], tasks: &TaskMap) -> (bool, String) {
    let req: UrlRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return (false, json_error(&format!("请求体不是有效 JSON: {}", e))),
    };
    let key = video_key(req.url.trim());
    let mut map = tasks.write().await;
    match map.get(&key) {
        Some(e) if is_active_state(&e.state.state) => {
            (false, json_error("任务进行中，请先取消再移除"))
        }
        Some(_) => {
            map.remove(&key);
            (true, r#"{"ok":true}"#.to_string())
        }
        None => (false, json_error("任务不存在")),
    }
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
    async fn test_cancel_forget_status() {
        let tasks: TaskMap = Arc::new(RwLock::new(HashMap::new()));
        let signal = TaskSignal::new();
        tasks.write().await.insert(
            "u1".to_string(),
            TaskEntry {
                state: TaskState {
                    url: "u1".to_string(),
                    name: "n1".to_string(),
                    state: "queued".to_string(),
                    segments: 3,
                    error: String::new(),
                    started_at: 1,
                    finished_at: 0,
                },
                signal: Arc::clone(&signal),
            },
        );

        // 取消：置标志 + 状态转 cancelling（幂等，可重复取消）
        let (ok, _) = handle_cancel(br#"{"url":"u1"}"#, &tasks).await;
        assert!(ok);
        assert!(signal.is_cancelled());
        assert_eq!(
            tasks.read().await.get("u1").unwrap().state.state,
            "cancelling"
        );
        let (ok2, _) = handle_cancel(br#"{"url":"u1"}"#, &tasks).await;
        assert!(ok2, "cancelling 仍是活动状态，重复取消应幂等成功");

        // 取消不存在的任务报错
        let (ok3, _) = handle_cancel(br#"{"url":"none"}"#, &tasks).await;
        assert!(!ok3);

        // forget：活动任务被拒；落终态后可移除
        let (ok4, _) = handle_forget(br#"{"url":"u1"}"#, &tasks).await;
        assert!(!ok4);
        finalize_task(&tasks, "u1", "cancelled", "").await;
        let (ok5, _) = handle_forget(br#"{"url":"u1"}"#, &tasks).await;
        assert!(ok5);
        assert!(tasks.read().await.get("u1").is_none());

        // status：插入两条任务，检查结构与计数
        tasks.write().await.insert(
            "a".to_string(),
            TaskEntry {
                state: TaskState {
                    url: "a".to_string(),
                    name: "na".to_string(),
                    state: "done".to_string(),
                    segments: 1,
                    error: String::new(),
                    started_at: 2,
                    finished_at: 3,
                },
                signal: TaskSignal::new(),
            },
        );
        tasks.write().await.insert(
            "b".to_string(),
            TaskEntry {
                state: TaskState {
                    url: "b".to_string(),
                    name: "nb".to_string(),
                    state: "downloading".to_string(),
                    segments: 9,
                    error: String::new(),
                    started_at: 3,
                    finished_at: 0,
                },
                signal: TaskSignal::new(),
            },
        );
        let text = handle_status(&tasks).await;
        let v: serde_json::Value = serde_json::from_str(&text).expect("status 应为合法 JSON");
        assert_eq!(v["ok"], true);
        assert_eq!(v["counts"]["done"], 1);
        assert_eq!(v["counts"]["downloading"], 1);
        assert_eq!(v["tasks"].as_array().unwrap().len(), 2);
        // 按开始时间倒序：b(3) 在 a(2) 前；进度字段始终存在
        assert_eq!(v["tasks"][0]["url"], "b");
        assert_eq!(v["tasks"][0]["segments_done"], 0);
        assert_eq!(v["tasks"][0]["segments"], 9);
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
