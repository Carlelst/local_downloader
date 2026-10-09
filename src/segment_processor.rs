use anyhow::{anyhow, Result};
use log::{debug, error, info, trace, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

use crate::{decrypt_segment, download_with_retry};

const REPORT_INTERVAL: Duration = Duration::from_secs(30);
const JPEG_CONVERT_TIMEOUT: Duration = Duration::from_secs(30);

/// 将 JPEG 图片转换为 MPEG-TS 格式
///
/// # 限制
/// - 每个 JPEG 段会独立 spawn 一个 ffmpeg 进程（N 段 → N 次启动）
/// - 已知性能瓶颈：未来可通过长生命周期 ffmpeg + 自定义协议优化
/// - 已加 30s 超时，防止 ffmpeg 异常卡死冻死 segment worker
async fn convert_jpeg_to_ts(jpeg_data: &[u8]) -> Result<Vec<u8>> {
    let mut child = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("mjpeg")
        .arg("-i")
        .arg("pipe:0")
        .arg("-f")
        .arg("mpegts")
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-preset")
        .arg("fast")
        .arg("-crf")
        .arg("23")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // 关键：stderr=DEVNULL 避免 ffmpeg 在 tcsetattr/ioctl 上永久阻塞
        // （ffmpeg 启动时尝试设置 TTY，pipe 不是 TTY 会卡死）
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("启动 FFmpeg 转换进程失败: {}", e))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("无法打开 FFmpeg stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("无法打开 FFmpeg stdout"))?;

    // 并行写 stdin / 读 stdout：避免输出超过 pipe 缓冲区（64KB）时
    // ffmpeg 阻塞在写 stdout 而我们阻塞在 wait 的经典死锁
    let jpeg_data_clone = jpeg_data.to_vec();
    let writer = tokio::spawn(async move {
        let result = stdin.write_all(&jpeg_data_clone).await;
        // 关闭 stdin 通知 ffmpeg 输入结束（必须 drop/shutdown 才能让 ffmpeg 退出）
        let _ = stdin.shutdown().await;
        result
    });

    let converted = tokio::time::timeout(JPEG_CONVERT_TIMEOUT, async {
        let mut ts_buf = Vec::with_capacity(jpeg_data.len());
        stdout
            .read_to_end(&mut ts_buf)
            .await
            .map_err(|e| anyhow!("读取 FFmpeg 输出失败: {}", e))?;
        writer
            .await
            .map_err(|e| anyhow!("写入任务 JoinError: {}", e))?
            .map_err(|e| anyhow!("写入 JPEG 数据失败: {}", e))?;
        let status = child
            .wait()
            .await
            .map_err(|e| anyhow!("等待 FFmpeg 进程失败: {}", e))?;
        if !status.success() {
            return Err(anyhow!("FFmpeg 转换退出码: {}", status));
        }
        Ok(ts_buf)
    })
    .await;

    match converted {
        Ok(result) => result,
        Err(_) => {
            // 超时：强制 kill，防止 ffmpeg 卡死
            let _ = child.kill().await;
            Err(anyhow!(
                "JPEG 转换超时（{}s）已 kill",
                JPEG_CONVERT_TIMEOUT.as_secs()
            ))
        }
    }
}

/// RFC 8216 §5.2：无显式 IV 时，用分片的 media sequence number 作为 IV
/// （16 字节大端：高 8 字节为零，低 8 字节为序号）
pub fn media_sequence_iv(seq: u64) -> Vec<u8> {
    let mut iv = vec![0u8; 16];
    iv[8..].copy_from_slice(&seq.to_be_bytes());
    iv
}

/// 校验分片数据签名：TS 以 0x47 同步字节开头；fMP4 段是 ISO-BMFF box 结构
/// （box header = 4 字节大端 size + 4 字节 ASCII type，如 moof/sidx/styp/emsg，
/// 不同 CDN 首个 box 不固定，因此做结构校验而非枚举类型）
pub(crate) fn is_valid_segment_signature(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    if data[0] == 0x47 {
        // 追加校验后续 TS 包同步字节（188 字节定长包），拦截首字节碰巧为 0x47 的垃圾数据
        if data.len() > 188 && data[188] != 0x47 {
            return false;
        }
        if data.len() > 376 && data[376] != 0x47 {
            return false;
        }
        return true;
    }
    // HLS 允许 TS 分片以 ID3 tag 开头（0x49 0x44 0x33），跳过 tag 后应为 0x47
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let tag_size = ((data[6] as usize & 0x7f) << 21)
            | ((data[7] as usize & 0x7f) << 14)
            | ((data[8] as usize & 0x7f) << 7)
            | (data[9] as usize & 0x7f);
        let ts_offset = 10 + tag_size;
        if data.len() == ts_offset {
            return true; // 仅含 ID3 tag（无音视频数据），放行
        }
        return data.len() > ts_offset && data[ts_offset] == 0x47;
    }
    if data.len() < 8 {
        return false;
    }
    let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as u64;
    // box type 必须是 4 个可打印 ASCII 字符（ISO-BMFF 规范）
    if !data[4..8].iter().all(|c| (0x20..=0x7e).contains(c)) {
        return false;
    }
    // size 合法性：>=8 为普通 box；==1 表示后跟 8 字节 largesize；==0 表示延伸到文件末尾
    match size {
        1 => data.len() >= 16 && data[8..16].iter().any(|&b| b != 0),
        0 => true,
        s if s >= 8 => true,
        _ => false,
    }
}

// 使用Mutex替代unsafe静态变量
lazy_static::lazy_static! {
    static ref STATS: Mutex<Stats> = Mutex::new(Stats {
        last_report_time: None,
        total_processed: 0,
        total_size: 0,
    });
}

struct Stats {
    last_report_time: Option<Instant>,
    total_processed: usize,
    total_size: usize,
}

/// 处理一个视频片段：下载、解密并将结果保存到共享映射中
///
/// # 参数
/// - `client`: HTTP客户端
/// - `url`: 片段URL
/// - `idx`: 片段索引
/// - `aes_key`: 可选的AES密钥，用于解密
/// - `aes_iv`: 可选的AES初始化向量，用于解密
/// - `segments`: 共享的片段存储
/// - `notify_tx`: 用于通知处理完成的通道
/// - `timeout`: 超时（秒）
/// - `retries`: 重试次数
/// - `retry_interval`: 重试间隔（秒）
///
/// # 返回
/// 成功时返回处理后的数据
pub async fn process_segment(
    client: Arc<Client>,
    url: String,
    idx: usize,
    aes_key: Option<Vec<u8>>,
    aes_iv: Option<Vec<u8>>,
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>,
    notify_tx: Sender<usize>,
    timeout: u64,
    retries: u8,
    retry_interval: u64,
    media_seq: u64,
) -> Result<Vec<u8>> {
    let start_time = Instant::now();
    debug!("开始处理片段 {}: {}", idx, url);

    // 创建下载进度回调
    let download_start = Instant::now();
    let last_progress_time = Arc::new(Mutex::new(Instant::now()));
    let downloaded_bytes = Arc::new(Mutex::new(0usize));

    let downloaded_bytes_clone = Arc::clone(&downloaded_bytes);
    let last_time_clone = Arc::clone(&last_progress_time);

    let progress_callback = Box::new(move |offset: usize, chunk_size: usize, _chunk: &[u8]| {
        let mut bytes = downloaded_bytes_clone.lock().unwrap();
        *bytes = offset + chunk_size;

        // 每5秒更新一次进度，避免日志过多
        let mut last_time = last_time_clone.lock().unwrap();
        if last_time.elapsed() > Duration::from_secs(5) {
            let elapsed = download_start.elapsed();
            let speed = if elapsed.as_millis() > 0 {
                *bytes as f64 / elapsed.as_millis() as f64 * 1000.0 / 1024.0
            } else {
                0.0
            };
            trace!("片段 {} 下载进度: {} 字节, {:.2} KB/s", idx, *bytes, speed);
            *last_time = Instant::now();
        }
    });

    // 下载片段
    let data = download_with_retry(
        &client,
        &url,
        timeout,
        retries,
        retry_interval,
        Some(progress_callback),
    )
    .await
    .map_err(|e| {
        error!("下载片段 {} 失败: {}", idx, e);
        anyhow!("下载片段失败 ({}): {}", idx, e)
    })?;

    // 更新统计信息
    {
        let mut stats = STATS.lock().unwrap();
        stats.total_size += data.len();
        stats.total_processed += 1;

        // 如果需要，打印定期报告
        let now = Instant::now();
        if stats.last_report_time.is_none() {
            stats.last_report_time = Some(now);
        } else if let Some(last) = stats.last_report_time {
            if now.duration_since(last) >= REPORT_INTERVAL {
                info!(
                    "性能报告: 已处理 {} 个片段, 总大小 {:.2} MB",
                    stats.total_processed,
                    stats.total_size as f64 / (1024.0 * 1024.0)
                );
                stats.last_report_time = Some(now);
            }
        }
    }

    // 加密流的数据完整性预检：AES-CBC 密文长度必须是 16 的倍数
    // （防止把被截断/注入垃圾的密文当作合法数据落盘）
    if aes_key.is_some()
        && !url.ends_with(".jpg")
        && !url.ends_with(".jpeg")
        && data.len() % 16 != 0
    {
        return Err(anyhow!(
            "片段 {} 密文长度 {} 不是 16 的倍数，数据在传输中损坏（可能被服务器注入垃圾或截断）",
            idx,
            data.len()
        ));
    }

    // 解密数据（如果需要）
    let processed_data = if let Some(key) = &aes_key {
        // 检查是否是图片文件，如果是则跳过解密
        if url.ends_with(".jpg") || url.ends_with(".jpeg") {
            debug!("跳过图片片段 {} 的解密步骤", idx);
            data.clone()
        } else {
            // RFC 8216 §5.2：无显式 IV 时，每个分片用自己的 media sequence number 作 IV
            let iv = match &aes_iv {
                Some(iv) => iv.clone(),
                None => {
                    let seq = media_seq + idx as u64;
                    info!(
                        "片段 {} 无显式 IV，使用 media sequence number ({}) 作为 IV",
                        idx, seq
                    );
                    media_sequence_iv(seq)
                }
            };
            debug!("解密片段 {}, 大小: {} 字节", idx, data.len());
            match decrypt_segment(&data, key, &iv) {
                Ok(decrypted) => {
                    debug!(
                        "解密成功，片段 {} 解密后大小: {} 字节",
                        idx,
                        decrypted.len()
                    );
                    decrypted
                }
                Err(e) => {
                    // 解密失败意味着数据损坏，绝不能把密文当结果保存
                    return Err(anyhow!(
                        "解密片段 {} 失败: {}（数据已损坏，需重新下载）",
                        idx,
                        e
                    ));
                }
            }
        }
    } else {
        trace!("片段 {} 不需要解密", idx);
        data.clone()
    };

    // 数据签名校验：TS 以 0x47 同步字节开头；fMP4 段以 box size(4字节) + "styp"/"ftyp" 开头
    // 拦截"长度正确但内容被污染"的下载（如服务器防盗链注入垃圾数据）
    if !url.ends_with(".jpg")
        && !url.ends_with(".jpeg")
        && !is_valid_segment_signature(&processed_data)
    {
        return Err(anyhow!(
            "片段 {} 数据校验失败: 首字节 0x{:02x} 不是合法的 TS/fMP4 签名，内容疑似被污染",
            idx,
            processed_data.first().copied().unwrap_or(0)
        ));
    }

    // 将 JPEG 图片转换为 MPEG-TS 格式
    let processed_data = if url.ends_with(".jpg") || url.ends_with(".jpeg") {
        match convert_jpeg_to_ts(&processed_data).await {
            Ok(ts_data) => {
                debug!(
                    "JPEG 转换 TS 成功，片段 {} 转换后大小: {} 字节",
                    idx,
                    ts_data.len()
                );
                ts_data
            }
            Err(e) => {
                // 转换失败的数据以 .ts 喂给 mpegts 管道必然产坏文件，
                // 与解密失败同理：报错交给上层重试重新下载，绝不回退存原始数据
                return Err(anyhow!(
                    "JPEG 转换 TS 失败（片段 {}）: {}（数据无效，需重新下载）",
                    idx,
                    e
                ));
            }
        }
    } else {
        processed_data
    };

    // 注意片段大小（大小异常时警告，正常时静默）
    if processed_data.len() != data.len() {
        debug!(
            "片段 {} 处理完成: {} → {} 字节，用时: {:.2?}",
            idx,
            data.len(),
            processed_data.len(),
            start_time.elapsed()
        );
    } else {
        trace!(
            "片段 {} 处理完成: {} 字节，用时: {:.2?}",
            idx,
            processed_data.len(),
            start_time.elapsed()
        );
    }

    // 将处理后的数据插入共享Map
    {
        let mut segments_lock = segments.write().await;
        segments_lock.insert(idx, processed_data.clone());
    }

    // 通知处理完成
    if let Err(e) = notify_tx.send(idx).await {
        warn!("无法发送完成通知: {}, idx: {}", e, idx);
    }

    Ok(processed_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    // ---------- 签名校验（对应 fMP4 偏移修复） ----------

    #[test]
    fn test_signature_ts_sync_byte() {
        let ts = vec![0x47u8; 188];
        assert!(is_valid_segment_signature(&ts));
    }

    #[test]
    fn test_signature_fmp4_styp_box() {
        // 合法 fMP4 段: 4字节 box size + "styp"（box type 在偏移 4-8）
        let mut seg = vec![0u8, 0, 0, 24];
        seg.extend_from_slice(b"styp");
        seg.extend_from_slice(&[0u8; 16]);
        assert!(is_valid_segment_signature(&seg));
    }

    #[test]
    fn test_signature_fmp4_moof_box_no_styp() {
        // eporner 等站点的 fMP4 分片直接以 moof 开头（无 styp 前缀）
        // 真实数据样例: 00 00 0f b0 "moof" 00 00 00 10 "mfhd" ...
        let mut seg = vec![0x00, 0x00, 0x0f, 0xb0];
        seg.extend_from_slice(b"moof");
        seg.extend_from_slice(&[0u8, 0, 0, 16]);
        seg.extend_from_slice(b"mfhd");
        seg.extend_from_slice(&[0u8; 12]);
        assert!(is_valid_segment_signature(&seg));
    }

    #[test]
    fn test_signature_fmp4_other_first_boxes() {
        // sidx / prft / emsg / free 等合法首 box 均应通过
        for typ in ["sidx", "prft", "emsg", "free"] {
            let mut seg = vec![0u8, 0, 0, 40];
            seg.extend_from_slice(typ.as_bytes());
            seg.extend_from_slice(&[0u8; 32]);
            assert!(is_valid_segment_signature(&seg), "{} 应通过", typ);
        }
    }

    #[test]
    fn test_signature_fmp4_edge_sizes() {
        // size==0：box 延伸到文件末尾
        let mut seg = vec![0u8, 0, 0, 0];
        seg.extend_from_slice(b"moof");
        seg.extend_from_slice(&[0u8; 16]);
        assert!(is_valid_segment_signature(&seg));
        // size==1：64 位 largesize 跟在 type 后
        let mut seg64 = vec![0u8, 0, 0, 1];
        seg64.extend_from_slice(b"moof");
        seg64.extend_from_slice(&[0u8; 8]); // 8 字节 largesize，非零
        seg64[8] = 0x40;
        assert!(is_valid_segment_signature(&seg64));
        // size < 8 非法（如 0x00000002）
        let mut bad = vec![0u8, 0, 0, 2];
        bad.extend_from_slice(b"moof");
        bad.extend_from_slice(&[0u8; 16]);
        assert!(!is_valid_segment_signature(&bad));
    }

    #[test]
    fn test_signature_rejects_corrupted_data() {
        // 模拟服务器注入垃圾数据：高熵首字节，box type 位置非可打印 ASCII
        let garbage: Vec<u8> = (0..1024).map(|i| (i * 37 % 251) as u8).collect();
        assert!(!is_valid_segment_signature(&garbage));
        // 密文（全随机）也不应通过
        assert!(!is_valid_segment_signature(&[
            0xab, 0x0f, 0xc4, 0x45, 0x83, 0xb2, 0xf2, 0x00
        ]));
        // 首字节 0x00 且 box type 位置为 NUL 的损坏数据
        assert!(!is_valid_segment_signature(&[
            0x00, 0x00, 0x0f, 0xb0, 0x00, 0x00, 0x00, 0x00
        ]));
    }

    #[test]
    fn test_signature_short_and_empty() {
        assert!(!is_valid_segment_signature(&[]));
        assert!(!is_valid_segment_signature(&[0x12, 0x34, 0x56])); // < 8 字节且非 0x47
                                                                   // 恰好 8 字节的合法 styp
        let minimal = [0u8, 0, 0, 24, b's', b't', b'y', b'p'];
        assert!(is_valid_segment_signature(&minimal));
    }

    // ---------- JPEG → TS 转换（对应 tokio 异步管道修复） ----------

    fn ffmpeg_available() -> bool {
        StdCommand::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 用 ffmpeg 生成一帧真实 JPEG（避免硬编码不完整的 JPEG 字节）
    fn generate_test_jpeg() -> Option<Vec<u8>> {
        let output = StdCommand::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "color=red:s=64x64",
                "-frames:v",
                "1",
                "-f",
                "mjpeg",
                "-",
            ])
            .output()
            .ok()?;
        if output.status.success() && output.stdout.starts_with(&[0xFF, 0xD8]) {
            Some(output.stdout)
        } else {
            None
        }
    }

    #[tokio::test]
    async fn test_convert_jpeg_to_ts_success() {
        if !ffmpeg_available() {
            eprintln!("跳过: 系统无 ffmpeg");
            return;
        }
        let Some(jpeg) = generate_test_jpeg() else {
            eprintln!("跳过: 无法生成测试 JPEG");
            return;
        };
        // 修复前：输出超过 64KB pipe 缓冲会死锁 → 30s 超时 kill
        // 修复后：并行读 stdout，正常返回 TS 数据
        let ts = convert_jpeg_to_ts(&jpeg).await.expect("JPEG→TS 转换应成功");
        assert!(!ts.is_empty());
        assert_eq!(ts[0], 0x47, "输出应为合法 MPEG-TS（0x47 同步字节）");
    }
}
