//! rou.video 家族伪装流下载
//!
//! 该家族把 HLS 内容伪装成图片或任意扩展名文件，实测存在三种变体，
//! 因此统一按**内容探测**解码，不依赖 URL 扩展名：
//!
//! 1. PNG 伪装：文件头为合法 PNG，数据藏在自定义 `roUd` chunk 的 payload 中，
//!    payload 首字节为类型标记：`0x00` + MPEG-TS（视频分片）、
//!    `0x01` + zlib（zlib 压缩的 m3u8 playlist）
//! 2. 明文 playlist：内容直接是 m3u8 文本（假 .jpg 等扩展名）
//! 3. 裸 TS 分片：内容直接是 MPEG-TS 数据（假 .jpg 等扩展名）
//!
//! playlist 中的分片为**绝对 URL**（命名如 `CLS-1-v1-a1.png` / `C-000.jpg`），
//! 分散在多个子域节点（gokj68.xyz / monb18.xyz / v.rn2xx.xyz 等），
//! 每片携带独立 auth 参数，因此直接按 playlist 逐片下载即可，无需构造或枚举 URL。
//!
//! 输入 URL 填 index.png / index.jpg（playlist 资源）的完整地址，程序自动：
//! 下载 → 内容探测解出 playlist → 并发下载全部分片 → 校验 → 合成 mp4。

use crate::downloader::download_with_retry;
use crate::ffmpeg::start_ffmpeg;
use crate::m3u8_parser::SegmentFormat;
use crate::Config;
use anyhow::{anyhow, Context, Result};
use log::{error, info, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Semaphore;

/// PNG 文件签名
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// 从 PNG 文件数据中提取 roUd chunk 的 payload
///
/// PNG 结构：8 字节签名后跟一系列 chunk，每个 chunk =
/// 4 字节大端长度 + 4 字节类型 + payload + 4 字节 CRC。
pub fn extract_roud(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return Err(anyhow!("校验失败: 不是有效的 PNG 文件（伪装分片已损坏）"));
    }
    let mut offset = 8usize;
    while offset + 8 <= data.len() {
        let len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = &data[offset + 4..offset + 8];
        let payload_start = offset + 8;
        let payload_end = payload_start
            .checked_add(len)
            .ok_or_else(|| anyhow!("PNG chunk 长度溢出"))?;
        if payload_end + 4 > data.len() {
            return Err(anyhow!("PNG chunk 越界，文件被截断"));
        }
        if chunk_type == b"roUd" {
            return Ok(data[payload_start..payload_end].to_vec());
        }
        // 跳过 payload + CRC
        offset = payload_end + 4;
    }
    Err(anyhow!("校验失败: PNG 中未找到 roUd 数据块"))
}

/// 下载内容解码结果
pub enum PngStreamResource {
    /// HLS playlist 文本
    Playlist(String),
    /// MPEG-TS 视频分片（已通过签名校验）
    Ts(Vec<u8>),
}

/// 按内容探测解码下载的资源（不依赖 URL 扩展名）
///
/// 依次尝试：明文 m3u8 文本 → PNG + roUd（0x01+zlib playlist / 0x00+TS 分片）
/// → 裸 TS 分片。全部不匹配则报错（数据损坏或格式未知）。
pub fn decode_resource(data: &[u8]) -> Result<PngStreamResource> {
    // 1. 明文 playlist（假扩展名，如 index.jpg）
    if data.len() > 7 && &data[..7] == b"#EXTM3U" {
        return Ok(PngStreamResource::Playlist(
            String::from_utf8_lossy(data).into_owned(),
        ));
    }

    // 2. PNG 伪装
    if data.len() >= 8 && data[..8] == PNG_SIGNATURE {
        let payload = extract_roud(data)?;
        match payload.first() {
            // 0x01 + zlib = 压缩的 playlist
            Some(&0x01) => {
                let mut decoder = flate2::read::ZlibDecoder::new(&payload[1..]);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .context("playlist zlib 解压失败")?;
                let text = String::from_utf8(out).context("playlist 不是有效的 UTF-8")?;
                if !text.starts_with("#EXTM3U") {
                    return Err(anyhow!("校验失败: 解压后的 playlist 不以 #EXTM3U 开头"));
                }
                return Ok(PngStreamResource::Playlist(text));
            }
            // 0x00 + TS = 视频分片（strip 内含 0x47 直通判定）
            _ => {
                let ts = strip_ts_prefix(&payload)?;
                return Ok(PngStreamResource::Ts(ts));
            }
        }
    }

    // 3. 裸 TS 分片（假扩展名，如 C-000.jpg）
    if data.first() == Some(&0x47) {
        if data.len() % 188 == 0 {
            return Ok(PngStreamResource::Ts(data.to_vec()));
        }
        return Err(anyhow!(
            "校验失败: 裸 TS 长度 {} 不是 188 的倍数",
            data.len()
        ));
    }

    Err(anyhow!(
        "校验失败: 无法识别的内容格式（长度 {}，头部 {:02x?}）",
        data.len(),
        &data[..data.len().min(8)]
    ))
}

/// 从 m3u8 文本中按顺序提取分片 URL（非注释、非空行）
///
/// playlist 中的分片为绝对 URL（如 CLS-1-v1-a1.png，分散在多个子域，
/// 每片携带独立 auth）；若出现相对路径则基于 index.png 的 URL 拼接。
pub fn parse_segment_urls(playlist: &str, base_url: &str) -> Result<Vec<String>> {
    let mut urls = Vec::new();
    for line in playlist.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let url = url::Url::parse(line)
            .or_else(|_| {
                let base = url::Url::parse(base_url)?;
                base.join(line)
            })
            .with_context(|| format!("非法分片 URL: {}", line))?;
        urls.push(url.to_string());
    }
    if urls.is_empty() {
        return Err(anyhow!("playlist 中未找到任何分片"));
    }
    Ok(urls)
}

/// 剥离视频分片 roUd payload 的 1 字节类型前缀，返回纯 MPEG-TS 数据
///
/// 判定规则（实测验证）：
/// - payload[0] == 0x00 且 payload[1] == 0x47 且 (len-1) % 188 == 0 → 剥离类型字节
/// - payload[0] == 0x47 且 len % 188 == 0 → 无前缀，原样返回
/// - 其余情况视为数据损坏
pub fn strip_ts_prefix(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.is_empty() {
        return Err(anyhow!("校验失败: roUd payload 为空"));
    }
    // 标准 TS 包：188 字节定长，包首为 0x47 同步字节
    if payload[0] == 0x47 {
        if payload.len() % 188 != 0 {
            return Err(anyhow!(
                "校验失败: TS 数据长度 {} 不是 188 的倍数",
                payload.len()
            ));
        }
        return Ok(payload.to_vec());
    }
    if payload[0] == 0x00
        && payload.len() > 1
        && (payload.len() - 1) % 188 == 0
        && payload[1] == 0x47
    {
        return Ok(payload[1..].to_vec());
    }
    Err(anyhow!(
        "校验失败: roUd payload 不符合 TS 结构（长度 {}）",
        payload.len()
    ))
}

/// 下载 rou.video 家族伪装流并合成为视频文件
///
/// 流程：下载 playlist 资源（index.png / index.jpg）→ 内容探测解出 playlist
/// → 并发下载全部分片（内容探测逐片解码）→ 全部通过后按序写入 FFmpeg 合成 mp4。
/// 任一分片重试耗尽或校验失败则整体中止，绝不产出残缺文件。
///
/// # 参数
/// - `config`: 全局配置
/// - `client`: 已按域名配置好 Referer/Cookie 的 HTTP 客户端
/// - `url`: playlist 资源（index.png / index.jpg 等）的完整 URL（含 query 认证参数）
/// - `output_name`: 输出文件名（不含扩展名，调用方负责防撞命名）
/// - `cancel`: 取消标志（--server 模式传入；CLI 传 None）。在分片与合成检查点快速退出
///
/// # 返回
/// 输出文件完整路径
pub async fn download_png_stream(
    config: &Config,
    client: Arc<Client>,
    url: &str,
    output_name: &str,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<PathBuf> {
    let is_cancelled = || {
        cancel
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
    };

    // 取消检查点：入口
    if is_cancelled() {
        return Err(anyhow!("任务已取消"));
    }

    // 分片较大（~1-3MB），保底 120s 超时以兼容慢速代理
    let timeout_secs = config.segment_timeout.max(120);

    // 1. 下载 playlist 资源并按内容探测解码
    let data = download_with_retry(
        &client,
        url,
        timeout_secs,
        config.download_retries,
        config.retry_interval,
        None,
    )
    .await
    .map_err(|e| anyhow!("playlist 下载失败: {:#}", e))?;
    let playlist = match decode_resource(&data).map_err(|e| anyhow!("playlist 资源: {:#}", e))? {
        PngStreamResource::Playlist(text) => text,
        PngStreamResource::Ts(_) => {
            return Err(anyhow!(
                "输入 URL 是分片而非 playlist，请填 index.png/index.jpg 的地址: {}",
                url
            ));
        }
    };
    let segment_urls = parse_segment_urls(&playlist, url)?;
    let total = segment_urls.len();
    info!("playlist 解析完成，共 {} 个伪装分片", total);

    // 2. 并发下载全部分片，逐片提取 + 校验
    let segments: Arc<tokio::sync::RwLock<HashMap<usize, Vec<u8>>>> =
        Arc::new(tokio::sync::RwLock::new(HashMap::with_capacity(total)));
    let failed: Arc<tokio::sync::RwLock<Vec<usize>>> =
        Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads.max(1)));
    let completed = Arc::new(tokio::sync::RwLock::new(0usize));

    let mut tasks = Vec::with_capacity(total);
    for (idx, seg_url) in segment_urls.iter().enumerate() {
        let seg_url = seg_url.clone();
        let client = Arc::clone(&client);
        let segments = Arc::clone(&segments);
        let failed = Arc::clone(&failed);
        let semaphore = Arc::clone(&semaphore);
        let completed = Arc::clone(&completed);
        let retries = config.download_retries;
        let retry_interval = config.retry_interval;
        let cancel_task = cancel.clone();

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.expect("获取信号量失败");

            // 取消检查点：任务取消后快速失败，不再发起新分片下载
            if cancel_task
                .as_ref()
                .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            {
                failed.write().await.push(idx);
                return;
            }

            let result: Result<Vec<u8>> = async {
                let png = download_with_retry(
                    &client,
                    &seg_url,
                    timeout_secs,
                    retries,
                    retry_interval,
                    None,
                )
                .await
                .map_err(|e| anyhow!("分片 {} 下载失败: {:#}", idx, e))?;
                match decode_resource(&png).map_err(|e| anyhow!("分片 {}: {:#}", idx, e))? {
                    PngStreamResource::Ts(ts) => Ok(ts),
                    PngStreamResource::Playlist(_) => {
                        Err(anyhow!("分片 {} 返回了 playlist 而非视频数据", idx))
                    }
                }
            }
            .await;

            let done;
            match result {
                Ok(ts) => {
                    segments.write().await.insert(idx, ts);
                }
                Err(e) => {
                    error!("{:#}", e);
                    failed.write().await.push(idx);
                }
            }

            {
                let mut c = completed.write().await;
                *c += 1;
                done = *c;
            }
            if done % 10 == 0 || done == total {
                info!("PNG 分片进度: {}/{}", done, total);
            }
        });
        tasks.push(task);
    }
    for task in tasks {
        let _ = task.await;
    }

    // 3. 任一分片失败则整体中止（不产出残缺文件）
    let failed_idx = failed.read().await.clone();
    if !failed_idx.is_empty() {
        // 取消优先于失败判定：命中取消标志时按取消语义返回，上层据此清理
        if is_cancelled() {
            return Err(anyhow!("任务已取消"));
        }
        return Err(anyhow!(
            "{} 个分片下载/校验失败: {:?}，放弃合成（不产出残缺文件）",
            failed_idx.len(),
            failed_idx
        ));
    }
    if is_cancelled() {
        return Err(anyhow!("任务已取消"));
    }

    // 4. 按序写入 FFmpeg
    let (mut ffmpeg, output_file) = start_ffmpeg(config, SegmentFormat::Ts).await?;
    let write_result: Result<()> = async {
        let stdin = ffmpeg
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("无法打开 FFmpeg 标准输入"))?;
        let mut writer = BufWriter::with_capacity(config.buffer_size.max(1024 * 1024), stdin);
        let segments_lock = segments.read().await;
        for idx in 0..total {
            // 失败检查已保证全部存在
            let data = segments_lock.get(&idx).expect("分片数据缺失");
            if let Err(e) = writer.write_all(data).await {
                // FFmpeg 提前退出（如数据被污染导致解码中断）会表现为 broken pipe
                return Err(anyhow!("写入 FFmpeg 失败（分片 {}）: {}", idx, e));
            }
        }
        writer.flush().await?;
        Ok(())
    }
    .await;

    if let Err(e) = write_result {
        let _ = ffmpeg.start_kill();
        // 清理残缺输出
        let _ = tokio::fs::remove_file(&output_file).await;
        return Err(e);
    }

    // 关闭 stdin（drop ChildStdin 使 FFmpeg 收到 EOF，才会开始封装输出）
    drop(ffmpeg.stdin.take());

    match ffmpeg.wait().await {
        Ok(status) if status.success() => {
            // start_ffmpeg 用随机文件名，成功后重命名为语义化名称（防撞命名由调用方负责）
            let final_path = PathBuf::from(&config.output_directory)
                .join(format!("{}.{}", output_name, config.video_format));
            if let Err(e) = tokio::fs::rename(&output_file, &final_path).await {
                warn!(
                    "重命名输出文件失败 ({} -> {}): {}，保留随机名",
                    output_file,
                    final_path.display(),
                    e
                );
                return Ok(PathBuf::from(&output_file));
            }
            info!("PNG 伪装流下载完成: {}", final_path.display());
            Ok(final_path)
        }
        Ok(status) => {
            error!("FFmpeg 处理失败 ({}): {}", status, output_file);
            let _ = tokio::fs::remove_file(&output_file).await;
            Err(anyhow!("FFmpeg 处理失败: {}", output_file))
        }
        Err(e) => {
            warn!("等待 FFmpeg 退出失败: {}", e);
            Err(anyhow!("FFmpeg 进程异常: {}", e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造最小 PNG：签名 + IHDR + roUd + IEND
    fn build_png(roud_payload: &[u8]) -> Vec<u8> {
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIGNATURE);

        let push_chunk = |png: &mut Vec<u8>, chunk_type: &[u8; 4], payload: &[u8]| {
            png.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            png.extend_from_slice(chunk_type);
            png.extend_from_slice(payload);
            png.extend_from_slice(&[0u8; 4]); // CRC 占位（解析器不校验 CRC）
        };

        push_chunk(&mut png, b"IHDR", &[0u8; 13]);
        push_chunk(&mut png, b"roUd", roud_payload);
        push_chunk(&mut png, b"IEND", &[]);
        png
    }

    fn build_ts(n_packets: usize) -> Vec<u8> {
        let mut ts = Vec::with_capacity(n_packets * 188);
        for _ in 0..n_packets {
            let mut pkt = vec![0u8; 188];
            pkt[0] = 0x47;
            ts.extend_from_slice(&pkt);
        }
        ts
    }

    #[test]
    fn test_extract_roud_finds_payload() {
        let ts = build_ts(2);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&ts);
        let png = build_png(&payload);

        let extracted = extract_roud(&png).expect("应提取到 roUd");
        assert_eq!(extracted, payload);
    }

    #[test]
    fn test_extract_roud_rejects_non_png() {
        assert!(extract_roud(b"not a png").is_err());
        assert!(extract_roud(&[]).is_err());
    }

    #[test]
    fn test_extract_roud_missing_chunk() {
        // 无 roUd 的普通 PNG（签名 + IHDR + IEND）
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIGNATURE);
        for t in [b"IHDR", b"IEND"] {
            png.extend_from_slice(&13u32.to_be_bytes());
            png.extend_from_slice(&t[..]);
            png.extend_from_slice(&[0u8; 13]);
            png.extend_from_slice(&[0u8; 4]);
        }
        assert!(extract_roud(&png).is_err());
    }

    #[test]
    fn test_decode_resource_zlib_playlist_png() {
        let m3u8 = "#EXTM3U\n#EXTINF:10,\nhttps://example.com/CLS-1-v1-a1.png?auth=x\n";
        let mut payload = vec![0x01];
        {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(m3u8.as_bytes()).unwrap();
            payload.extend_from_slice(&encoder.finish().unwrap());
        }
        let png = build_png(&payload);
        match decode_resource(&png).expect("应解出 playlist") {
            PngStreamResource::Playlist(text) => assert_eq!(text, m3u8),
            PngStreamResource::Ts(_) => panic!("应为 Playlist"),
        }
    }

    #[test]
    fn test_decode_resource_plain_playlist() {
        let m3u8 = "#EXTM3U\nhttps://example.com/a.png\n";
        match decode_resource(m3u8.as_bytes()).expect("明文 m3u8 应直通") {
            PngStreamResource::Playlist(text) => assert_eq!(text, m3u8),
            PngStreamResource::Ts(_) => panic!("应为 Playlist"),
        }
    }

    #[test]
    fn test_decode_resource_png_ts_segment() {
        let ts = build_ts(2);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&ts);
        let png = build_png(&payload);
        match decode_resource(&png).expect("应解出 TS") {
            PngStreamResource::Ts(data) => assert_eq!(data, ts),
            PngStreamResource::Playlist(_) => panic!("应为 Ts"),
        }
    }

    #[test]
    fn test_decode_resource_bare_ts() {
        let ts = build_ts(3);
        match decode_resource(&ts).expect("裸 TS 应直通") {
            PngStreamResource::Ts(data) => assert_eq!(data, ts),
            PngStreamResource::Playlist(_) => panic!("应为 Ts"),
        }
        // 长度非 188 倍数的裸 TS 报错
        assert!(decode_resource(&ts[..ts.len() - 1]).is_err());
    }

    #[test]
    fn test_decode_resource_rejects_garbage() {
        assert!(decode_resource(b"\x02junk").is_err());
        assert!(decode_resource(&[]).is_err());
        // 是 PNG 但没有 roUd
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIGNATURE);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0u8; 13]);
        png.extend_from_slice(&[0u8; 4]);
        assert!(decode_resource(&png).is_err());
    }

    #[test]
    fn test_parse_segment_urls() {
        let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n\
            #EXTINF:10,\nhttps://v1.a.com/x/CLS-1-v1-a1.png?auth=A\n\
            \n\
            #EXTINF:10,\nhttps://v2.b.com/x/CLS-2-v1-a1.png?auth=B\n";
        let urls = parse_segment_urls(playlist, "https://g.com/index.png").expect("应解析成功");
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://v1.a.com/x/CLS-1-v1-a1.png?auth=A");
        assert_eq!(urls[1], "https://v2.b.com/x/CLS-2-v1-a1.png?auth=B");
    }

    #[test]
    fn test_parse_segment_urls_relative_and_empty() {
        // 相对路径基于 index URL 拼接；空 playlist 报错
        let urls = parse_segment_urls(
            "#EXTM3U\n#EXTINF:10,\nCLS-1.png?auth=A\n",
            "https://g.com/hls/abc/index.png?v=1",
        )
        .expect("相对路径应拼接");
        assert_eq!(urls[0], "https://g.com/hls/abc/CLS-1.png?auth=A");
        assert!(parse_segment_urls("#EXTM3U\n#EXTINF:10,\n", "https://g.com/i.png").is_err());
    }

    #[test]
    fn test_strip_ts_prefix_with_prefix() {
        let ts = build_ts(3);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&ts);
        let stripped = strip_ts_prefix(&payload).expect("应剥离前缀");
        assert_eq!(stripped, ts);
    }

    #[test]
    fn test_strip_ts_prefix_without_prefix() {
        let ts = build_ts(2);
        let stripped = strip_ts_prefix(&ts).expect("无前缀应原样返回");
        assert_eq!(stripped, ts);
    }

    #[test]
    fn test_strip_ts_prefix_rejects_corrupted() {
        // 长度不是 188 倍数
        let mut bad = vec![0x00];
        bad.extend_from_slice(&build_ts(1));
        bad.push(0xff);
        assert!(strip_ts_prefix(&bad).is_err());
        // 前缀 0x00 但第二字节不是 0x47
        let mut bad2 = vec![0x00, 0x12];
        bad2.extend_from_slice(&vec![0u8; 187]);
        assert!(strip_ts_prefix(&bad2).is_err());
        // 空数据
        assert!(strip_ts_prefix(&[]).is_err());
    }

    /// 集成测试：真实节点样本（gokj68 index.png，playlist 为 PNG+0x01+zlib）
    #[tokio::test]
    #[ignore]
    async fn test_real_sample_playlist() {
        let png = tokio::fs::read("/tmp/gokj_index.png")
            .await
            .expect("样本不存在");
        let playlist = match decode_resource(&png).expect("应解出 playlist") {
            PngStreamResource::Playlist(text) => text,
            PngStreamResource::Ts(_) => panic!("应为 Playlist"),
        };
        let urls = parse_segment_urls(&playlist, "https://gokj68.xyz/x/index.png").unwrap();
        assert!(urls.len() > 100);
        // URL 带独立 auth query，只比对路径段
        assert!(urls[0]
            .split('?')
            .next()
            .unwrap()
            .ends_with("CLS-1-v1-a1.png"));
    }

    /// 集成测试：rn245 变体（明文 m3u8 假 .jpg + 裸 TS 假 .jpg 分片）
    #[tokio::test]
    #[ignore]
    async fn test_real_sample_bare_playlist() {
        let jpg = tokio::fs::read("/tmp/rn245_index.jpg")
            .await
            .expect("样本不存在");
        let playlist = match decode_resource(&jpg).expect("应解出 playlist") {
            PngStreamResource::Playlist(text) => text,
            PngStreamResource::Ts(_) => panic!("应为 Playlist"),
        };
        let urls = parse_segment_urls(&playlist, "https://v.rn245.xyz/x/index.jpg").unwrap();
        assert_eq!(urls.len(), 85);
        assert!(urls[0].split('?').next().unwrap().ends_with("C-000.jpg"));
        // 分片本体是裸 TS
        let seg = tokio::fs::read("/tmp/rn245_seg0.bin")
            .await
            .expect("样本不存在");
        assert!(matches!(
            decode_resource(&seg),
            Ok(PngStreamResource::Ts(_))
        ));
    }
}
