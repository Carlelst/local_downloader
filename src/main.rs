use env_logger::Builder;
use log::{info, error, LevelFilter};
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Instant;
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

mod decryptor;

const MAX_CONCURRENT_DOWNLOADS: usize = 4;

async fn download_with_retry(client: &Client, url: &str, retries: u8) -> Result<Vec<u8>> {
    let mut attempt = 0;
    while attempt <= retries {
        match client.get(url).timeout(Duration::from_secs(40)).send().await {
            Ok(response) if response.status().is_success() => {
                // info!("Successfully downloaded segment from {}", url);
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
            sleep(Duration::from_secs(3)).await; // 每次重试前等待 3 秒
        }
    }
    Err(anyhow::anyhow!("Failed to download segment from {} after {} retries", url, retries))
}


pub async fn parse_m3u8(m3u8_url: &str, client: &Client) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let response = client.get(m3u8_url).send().await?;
    let m3u8_content = response.text().await?;

    // 提取 TS 文件的 URL 列表
    let ts_urls: Vec<String> = m3u8_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    // info!("Parsed TS URLs: {:?}", ts_urls);

    // 提取 AES 密钥的 URL 和可选的 IV
    let key_re = Regex::new(r#"EXT-X-KEY:METHOD=AES-128,URI="([^"]+)"(?:,IV=(0x[0-9a-fA-F]+))?"#)?;
    let (key_url, iv) = if let Some(captures) = key_re.captures(&m3u8_content) {
        let key_url = captures.get(1)
            .ok_or_else(|| anyhow::anyhow!("Key URL not found"))?
            .as_str()
            .to_string();

        let iv = if let Some(iv_match) = captures.get(2) {
            let iv_hex = &iv_match.as_str()[2..]; // 去掉 "0x" 前缀
            hex::decode(iv_hex).ok()
        } else {
            Some(vec![0; 16]) // 默认的零 IV
        };

        (Some(key_url), iv)
    } else {
        info!("M3U8 file does not contain AES key information");
        (None, None)
    };

    // 如果 key_url 存在则下载 AES 密钥
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
    let m3u8_url = "https://jid45g65d4qo.cdn-jupiter.com/hls2/01/04745/fqnd6g9i9vym_,l,n,h,.urlset/index-f2-v1-a1.m3u8?t=TDspJK7dbXNHyjcV4GdBJFbm8z0Esu70-3nQH_tkw1w&s=1730662740&e=129600&f=23727860&srv=07s7cile1o9s&i=0.4&sp=500&p1=07s7cile1o9s&p2=07s7cile1o9s&asn=20473";

    let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
    info!("Parsed M3U8 file and retrieved AES key and IV(or default IV if missing)");

    let (tx, mut rx) = mpsc::channel(32);
    let mut ffmpeg = start_ffmpeg().await?;
    let ffmpeg_stdin = ffmpeg.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("Failed to open FFmpeg stdin"))?;
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

    for (idx, url) in ts_urls.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let tx = tx.clone();
        let aes_key = aes_key.clone();
        let aes_iv = aes_iv.clone();
        let semaphore = Arc::clone(&semaphore);

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let segment_start = Instant::now();

            match download_with_retry(&client, &url, 10).await {
                Ok(data) => {
                    info!("Downloaded segment {} in {:.2?}", idx, segment_start.elapsed());
                    let decrypt_start = Instant::now();
                    if let (Some(aes_key), Some(aes_iv)) = (&aes_key, &aes_iv) {
                        match decryptor::decrypt_segment(&data, aes_key, aes_iv) {
                            Ok(decrypted_data) => {
                                info!("Decrypted segment {} in {:.2?}", idx, decrypt_start.elapsed());
                                tx.send(decrypted_data).await.unwrap();
                            }
                            Err(e) => error!("Failed to decrypt segment {}: {:?}", idx, e),
                        }
                    } else {
                        // info!("No AES key or IV for segment {} - sending unencrypted data", idx);
                        tx.send(data).await.unwrap();
                    }
                }
                Err(e) => error!("Failed to download segment {} after retries: {:?}", idx, e),
            }
        });
    }

    drop(tx);

    while let Some(decrypted_data) = rx.recv().await {
        ffmpeg_stdin.write_all(&decrypted_data).await?;
    }

    ffmpeg_stdin.shutdown().await?;
    let ffmpeg_status = ffmpeg.wait().await?;
    if ffmpeg_status.success() {
        info!("FFmpeg process completed successfully");
    } else {
        error!("FFmpeg process encountered an error.");
    }

    info!("Total processing completed.");
    Ok(())
}
