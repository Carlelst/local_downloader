use env_logger::Builder;
use log::{info, error, LevelFilter};
use tokio::sync::{mpsc, Semaphore, RwLock};
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
        match client.get(url).timeout(Duration::from_secs(30)).send().await {
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

/// 将 JPEG 图像转换为 TS 格式
async fn convert_jpeg_to_ts(input_path: &str, output_path: &str, duration: u32) -> Result<()> {
    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loop").arg("1")
        .arg("-i").arg(input_path)
        .arg("-t").arg(duration.to_string()) // 设置每张图片的持续时间
        .arg("-c:v").arg("libx264")
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-f").arg("mpegts")
        .arg(output_path)
        .output()
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to convert JPEG to TS: {:?}", e))
}

/// 下载并处理片段（支持 TS 和 JPEG 转换）
async fn download_and_process_segment(client: Arc<Client>, url: String, idx: usize, 
    aes_key: Option<Vec<u8>>, aes_iv: Option<Vec<u8>>, 
    segments: Arc<RwLock<HashMap<usize, Vec<u8>>>>, 
    notify_tx: mpsc::Sender<usize>) -> Result<()> {
let is_jpeg = url.ends_with(".jpeg");

// 下载文件
let data = download_with_retry(&client, &url, 10).await?;

// 如果是 JPEG 文件，先保存为本地文件，再转换为 TS 格式
let processed_data = if is_jpeg {
let jpeg_path = format!("segment_{}.jpeg", idx);
let ts_path = format!("segment_{}.ts", idx);

tokio::fs::write(&jpeg_path, &data).await?;  // 保存 JPEG 文件
convert_jpeg_to_ts(&jpeg_path, &ts_path, 4).await?;  // 转换为 TS 格式

// 读取转换后的 TS 文件数据
tokio::fs::read(&ts_path).await?
} else {
data
};

// 解密 TS 数据（如果需要）
let decrypted_data = if let (Some(aes_key), Some(aes_iv)) = (&aes_key, &aes_iv) {
match decryptor::decrypt_segment(&processed_data, aes_key, aes_iv) {
Ok(decrypted_data) => decrypted_data,
Err(e) => {
error!("解密片段 {} 失败: {:?}", idx, e);
processed_data
}
}
} else {
processed_data
};

// 保存到共享 HashMap 中
let mut segments_lock = segments.write().await;
segments_lock.insert(idx, decrypted_data);
drop(segments_lock);

notify_tx.send(idx).await.unwrap();
Ok(())
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
    let m3u8_url = "https://g6las13cmjku.cdn-jupiter.com/hls2/01/02113/phh0hnk6fug5_n/index-v1-a1.m3u8?t=IPcZs08tCrOq7u4-REGVWZS_-Iu4-uiPsZ1p0ZFoB6M&s=1730730882&e=129600&f=10569821&srv=vr4pcjm18sa1&i=0.4&sp=500&p1=vr4pcjm18sa1&p2=vr4pcjm18sa1&asn=20473";
    let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
    info!("解析 M3U8 文件并获取了 AES 密钥和 IV（如果缺失则使用默认 IV）");

    let total_segments = ts_urls.len();
    let segments = Arc::new(RwLock::new(HashMap::<usize, Vec<u8>>::new()));
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
            if let Err(e) = download_and_process_segment(client, url, idx, aes_key, aes_iv, segments, notify_tx).await {
                error!("Failed to process segment {}: {:?}", idx, e);
            }
        });
    }

    drop(notify_tx);

    let mut next_idx = 0;
    while next_idx < total_segments {
        if let Some(_received_idx) = notify_rx.recv().await {
            let mut segments_lock = segments.write().await;
            while let Some(data) = segments_lock.remove(&next_idx) {
                ffmpeg_stdin.write_all(&data).await?;
                info!("写入片段 {} 到 FFmpeg", next_idx);
                next_idx += 1;
            }
            drop(segments_lock);
        } else {
            break;
        }
    }

    ffmpeg_stdin.shutdown().await?;
    let ffmpeg_status = ffmpeg.wait().await?;
    if ffmpeg_status.success() {
        info!("FFmpeg 处理完成");
    } else {
        error!("FFmpeg 进程处理失败");
    }

    info!("任务完成！");
    Ok(())
}
