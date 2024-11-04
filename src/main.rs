use env_logger::Builder;
use log::{info, error, LevelFilter};
use tokio::sync::{mpsc, Semaphore, Mutex};
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use reqwest::Client;
use regex::Regex;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use chrono::Local;
use std::io::Write;
use anyhow::Result;
use std::process::Stdio;
use std::collections::HashMap;

mod decryptor;

const MAX_CONCURRENT_DOWNLOADS: usize = 8;

async fn download_with_retry(client: &Client, url: &str, retries: u8) -> Result<Vec<u8>> {
    let mut attempt = 0;
    while attempt <= retries {
        match client.get(url).timeout(Duration::from_secs(40)).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(response.bytes().await?.to_vec());
            }
            Ok(response) => {
                error!("Failed to download segment {}: HTTP {}", url, response.status());
            }
            Err(e) if e.to_string().contains("end of file before message length reached") => {
                error!("Connection closed prematurely: {}", e);
            }
            Err(e) => {
                error!("Error downloading segment {}: {:?}", url, e);
            }
        }
        attempt += 1;
        if attempt <= retries {
            info!("Retrying download for {} (attempt {}/{})", url, attempt, retries);
            sleep(Duration::from_secs(3)).await;
        }
    }
    Err(anyhow::anyhow!("Failed to download segment from {} after {} retries", url, retries))
}

pub async fn parse_m3u8(m3u8_url: &str, client: &Client) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let response = client.get(m3u8_url).send().await?;
    let m3u8_content = response.text().await?;

    let ts_urls: Vec<String> = m3u8_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    let key_re = Regex::new(r#"EXT-X-KEY:METHOD=AES-128,URI="([^"]+)"(?:,IV=(0x[0-9a-fA-F]+))?"#)?;
    let (key_url, iv) = if let Some(captures) = key_re.captures(&m3u8_content) {
        let key_url = captures.get(1)
            .ok_or_else(|| anyhow::anyhow!("Key URL not found"))?
            .as_str()
            .to_string();

        let iv = if let Some(iv_match) = captures.get(2) {
            let iv_hex = &iv_match.as_str()[2..];
            hex::decode(iv_hex).ok()
        } else {
            Some(vec![0; 16])
        };

        (Some(key_url), iv)
    } else {
        info!("M3U8 file does not contain AES key information");
        (None, None)
    };

    let aes_key = if let Some(url) = key_url {
        Some(client.get(&url).send().await?.bytes().await?.to_vec())
    } else {
        None
    };

    Ok((ts_urls, aes_key, iv))
}

pub async fn start_ffmpeg() -> Result<tokio::process::Child> {
    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("mpegts")
        .arg("-i")
        .arg("pipe:0")
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("output.mp4")
        .stdin(Stdio::piped())
        .spawn()?;
    Ok(ffmpeg)
}

#[tokio::main]
async fn main() -> Result<()> {
    Builder::new()
        .filter(None, LevelFilter::Info)
        .format(|buf, record| {
            let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
            writeln!(buf, "[{} {}] {}", ts, record.level(), record.args())
        })
        .init();

    let client = Arc::new(Client::new());
    let m3u8_url = "https://anbgvvgzhm88.cdn-jupiter.com/hls2/01/01453/k9n8zgx639mm_n/index-v1-a1.m3u8?t=24D9mmZ80ihPtypK59IC_wLC6kQhbKaG3AA6JJgrsIk&s=1730731634&e=129600&f=7268461&srv=Gs5UDtPpa5&i=0.4&sp=500&p1=Gs5UDtPpa5&p2=Gs5UDtPpa5&asn=20473";
    let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
    info!("解析 M3U8 文件并获取了 AES 密钥和 IV（如果缺失则使用默认 IV）");

    let total_segments = ts_urls.len();
    let segments = Arc::new(Mutex::new(HashMap::<usize, Vec<u8>>::new()));
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

    let mut ffmpeg = start_ffmpeg().await?;
    let ffmpeg_stdin = ffmpeg.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("无法打开 FFmpeg 的标准输入"))?;

    let (notify_tx, mut notify_rx) = mpsc::channel::<usize>(total_segments);

    for (idx, url) in ts_urls.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let aes_key = aes_key.clone();
        let aes_iv = aes_iv.clone();
        let semaphore = Arc::clone(&semaphore);
        let segments = Arc::clone(&segments);
        let notify_tx = notify_tx.clone();

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            match download_with_retry(&client, &url, 10).await {
                Ok(data) => {
                    // 解密数据
                    let decrypted_data = if let (Some(aes_key), Some(aes_iv)) = (&aes_key, &aes_iv) {
                        match decryptor::decrypt_segment(&data, aes_key, aes_iv) {
                            Ok(decrypted_data) => {
                                info!("解密片段 {} 成功", idx);
                                decrypted_data
                            }
                            Err(e) => {
                                error!("解密片段 {} 失败: {:?}", idx, e);
                                data
                            }
                        }
                    } else {
                        data
                    };

                    // 将解密后的数据存储到共享的 HashMap 中
                    let mut segments_lock = segments.lock().await;
                    segments_lock.insert(idx, decrypted_data);
                    drop(segments_lock);

                    // 通知主线程该片段已处理完成
                    notify_tx.send(idx).await.unwrap();
                }
                Err(e) => error!("Download {} Failed: {:?}", idx, e),
            }
        });
    }

    drop(notify_tx); // 关闭发送端

    // 主线程按顺序写入 ffmpeg
    let mut next_idx = 0;
    while next_idx < total_segments {
        // 等待通知
        if let Some(_received_idx) = notify_rx.recv().await {
            let mut segments_lock = segments.lock().await;
            // 检查是否有下一个需要的片段
            while let Some(data) = segments_lock.remove(&next_idx) {
                // 写入 ffmpeg
                ffmpeg_stdin.write_all(&data).await?;
                info!("Wr {} in ffmpeg", next_idx);
                next_idx += 1;
            }
            drop(segments_lock);
        } else {
            break; // 所有发送者都已关闭
        }
    }

    ffmpeg_stdin.shutdown().await?;
    let ffmpeg_status = ffmpeg.wait().await?;
    if ffmpeg_status.success() {
        info!("FFmpeg finish");
    } else {
        error!("FFmpeg process error");
    }

    info!("Done!");
    Ok(())
}
