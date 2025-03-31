use anyhow::{anyhow, Result};
use chrono::Local;
use env_logger::Builder;
use log::{debug, error, info, warn};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::{sleep, Duration, Instant};

use video_processor::{
    ffmpeg::start_ffmpeg, m3u8_parser::parse_m3u8, parse_log_level,
    segment_processor::process_segment,
};

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    let config = video_processor::load_config()?;

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

    let client = Client::builder()
        .timeout(Duration::from_secs(config.segment_timeout))
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(10)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;
    let client = Arc::new(client);

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

        info!(
            "处理 M3U8 文件 [{}/{}]: {}",
            i + 1,
            config.m3u8_urls.len(),
            m3u8_url
        );

        // 计算一个基于URL的唯一文件名
        let url_hash = format!("{:x}", md5::compute(m3u8_url));
        let base_filename = url_hash[..10].to_string();
        let metadata_path = temp_dir.join(format!("{}.json", base_filename));
        let segments_dir = temp_dir.join(&base_filename);
        if !segments_dir.exists() {
            create_dir_all(&segments_dir).await?;
        }

        // 解析M3U8获取片段链接和加密信息
        info!("解析M3U8: {}", m3u8_url);
        let (ts_urls, aes_key, aes_iv) = parse_m3u8(m3u8_url, &client).await?;
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

            // 启动下载任务
            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("获取信号量失败");
                match process_segment(
                    client,
                    url,
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
                                debug!("成功下载并保存片段 {}", idx);
                            }
                        }
                        to_download.write().await.remove(&idx);
                    }
                    Err(e) => {
                        error!("片段 {} 处理失败: {:?}", idx, e);
                    }
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
        let (mut ffmpeg, output_file) = start_ffmpeg(&config).await?;
        let ffmpeg_stdin = ffmpeg
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("无法打开 FFmpeg 标准输入"))?;
        let mut buffered_stdin = BufWriter::with_capacity(config.buffer_size, ffmpeg_stdin);

        // 进度追踪并写入FFmpeg
        let mut processed_segments = 0;
        let mut next_idx = 0;
        let mut processed_indices = HashSet::new();

        // 有两部分工作：
        // 1. 将已下载的片段按顺序写入FFmpeg
        // 2. 等待新下载的片段通知并写入FFmpeg

        // 先尝试按顺序写入已下载的片段
        let mut segments_lock = segments.write().await;
        while let Some(data) = segments_lock.remove(&next_idx) {
            debug!("写入已缓存的片段 {} 到 FFmpeg", next_idx);
            buffered_stdin.write_all(&data).await?;
            processed_indices.insert(next_idx);
            processed_segments += 1;
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
                debug!("写入片段 {} 到 FFmpeg", next_idx);
                buffered_stdin.write_all(&data).await?;
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

        // 检查是否有缺失的片段
        let mut missing_segments = Vec::new();
        for i in 0..total_segments {
            if !processed_indices.contains(&i) {
                missing_segments.push(i);
            }
        }

        // 等待所有下载任务完成
        for task in download_tasks {
            let _ = task.await;
        }

        // 等待FFmpeg处理完成
        if ffmpeg.wait().await?.success() {
            info!("FFmpeg 处理成功: {}", output_file);

            // 如果处理成功但有片段缺失，重命名文件以标明不完整
            if !missing_segments.is_empty() {
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
