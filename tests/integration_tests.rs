use reqwest::Client;
use tokio::sync::Semaphore;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use video_processor::downloader::download_with_retry;
use video_processor::m3u8_parser::parse_m3u8;
use video_processor::ffmpeg::start_ffmpeg;
use video_processor::decryptor;

#[tokio::test]
async fn test_full_download_and_process() {
    let client = Arc::new(Client::new());
    let m3u8_url = "https://anbgvvgzhm88.cdn-jupiter.com/hls2/01/01453/k9n8zgx639mm_n/index-v1-a1.m3u8?t=24D9mmZ80ihPtypK59IC_wLC6kQhbKaG3AA6JJgrsIk&s=1730731634&e=129600&f=7268461&srv=Gs5UDtPpa5&i=0.4&sp=500&p1=Gs5UDtPpa5&p2=Gs5UDtPpa5&asn=20473";

    let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await.expect("M3U8 解析失败");
    let semaphore = Arc::new(Semaphore::new(8));  // 确保设置了正确的并发限制

    for url in ts_urls.iter() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let data = download_with_retry(&client, url, 3).await.expect("下载失败");

            if let Some(key) = aes_key.as_ref() {
                decryptor::decrypt_segment(&data, key, aes_iv.as_ref().unwrap()).expect("解密失败");
            }
        }).await.unwrap();
    }

    let mut ffmpeg = start_ffmpeg().await.expect("FFmpeg 启动失败");
    let ffmpeg_stdin = ffmpeg.stdin.as_mut().expect("无法打开 FFmpeg 的标准输入");

    for url in ts_urls {
        let data = download_with_retry(&client, &url, 3).await.unwrap();
        ffmpeg_stdin.write_all(&data).await.expect("写入 FFmpeg 失败");
    }

    ffmpeg_stdin.shutdown().await.expect("FFmpeg 输入流关闭失败");
    let status = ffmpeg.wait().await.expect("等待 FFmpeg 进程失败");
    assert!(status.success(), "FFmpeg 进程未成功完成");
}
