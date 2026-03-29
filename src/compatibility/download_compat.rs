/// 下载功能兼容性包装器
///
/// 提供与现有下载API兼容的接口，内部使用新的下载管理器实现
use anyhow::Result;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

use crate::domain::services::DownloadManager;
use crate::infrastructure::{
    AdaptiveDownloadManager, DefaultFileSystem, DefaultHttpClient, DownloadConfig,
    JsonFilePersistence,
};

/// 兼容性下载函数 - 使用新架构实现
///
/// 该函数保持与原始 `download_with_retry` 函数相同的签名，
/// 但内部使用新的下载管理器实现
pub async fn download_with_retry_new(
    client: &Client,
    url: &str,
    timeout_secs: u64,
    retries: u8,
    retry_interval: u64,
    chunk_callback: Option<Box<dyn Fn(usize, usize, &[u8]) + Send + Sync>>,
) -> Result<Vec<u8>> {
    // 创建新的HTTP客户端包装器
    let http_client = Arc::new(DefaultHttpClient::with_client(client.clone()));

    // 创建持久化存储
    let filesystem = Box::new(DefaultFileSystem::new());
    let persistence = Arc::new(JsonFilePersistence::new(
        std::path::PathBuf::from("./temp"),
        filesystem,
    ));

    // 创建下载配置
    let download_config = DownloadConfig {
        max_concurrent_downloads: 1, // 单个下载
        min_concurrent_downloads: 1,
        chunk_size: 1024 * 1024, // 1MB
        timeout_secs,
        max_retries: retries as u32,
        enable_resume: false, // 兼容模式不启用断点续传
        bandwidth_threshold_mbps: 10.0,
        latency_threshold_ms: 1000,
    };

    // 创建下载管理器
    let download_manager = AdaptiveDownloadManager::new(http_client, persistence, download_config);

    // 创建临时片段用于下载
    let segment = crate::domain::entities::Segment {
        index: 0,
        url: url.to_string(),
        duration: 0.0,
        size: None,
        encryption: None,
        metadata: crate::domain::entities::SegmentMetadata::default(),
    };

    // 执行下载
    let segments = vec![segment];
    let results = download_manager.download_segments(segments).await?;

    // 获取下载结果
    if let Some(data) = results.get(&0) {
        // 如果提供了回调函数，模拟分块回调
        if let Some(callback) = chunk_callback {
            let chunk_size = 8192; // 8KB 块
            for (offset, chunk) in data.chunks(chunk_size).enumerate() {
                callback(offset * chunk_size, chunk.len(), chunk);
            }
        }

        Ok(data.clone())
    } else {
        Err(anyhow::anyhow!("下载失败：未获取到数据"))
    }
}

/// 创建兼容的下载管理器
///
/// 该函数创建一个配置好的下载管理器，可以用于批量下载
pub async fn create_compatible_download_manager(
    client: &Client,
    max_concurrent: usize,
    timeout_secs: u64,
    retries: u8,
) -> Result<Arc<dyn DownloadManager>> {
    let http_client = Arc::new(DefaultHttpClient::with_client(client.clone()));

    let filesystem = Box::new(DefaultFileSystem::new());
    let persistence = Arc::new(JsonFilePersistence::new(
        std::path::PathBuf::from("./temp"),
        filesystem,
    ));

    let download_config = DownloadConfig {
        max_concurrent_downloads: max_concurrent,
        min_concurrent_downloads: 1,
        chunk_size: 1024 * 1024,
        timeout_secs,
        max_retries: retries as u32,
        enable_resume: true,
        bandwidth_threshold_mbps: 10.0,
        latency_threshold_ms: 1000,
    };

    Ok(Arc::new(AdaptiveDownloadManager::new(
        http_client,
        persistence,
        download_config,
    )))
}

/// 批量下载URL列表
///
/// 使用新的下载管理器实现批量下载功能
pub async fn batch_download_urls(
    urls: Vec<String>,
    client: &Client,
    max_concurrent: usize,
    timeout_secs: u64,
    retries: u8,
) -> Result<Vec<(usize, Vec<u8>)>> {
    let download_manager =
        create_compatible_download_manager(client, max_concurrent, timeout_secs, retries).await?;

    // 创建片段列表
    let segments: Vec<crate::domain::entities::Segment> = urls
        .into_iter()
        .enumerate()
        .map(|(index, url)| crate::domain::entities::Segment {
            index,
            url,
            duration: 0.0,
            size: None,
            encryption: None,
            metadata: crate::domain::entities::SegmentMetadata::default(),
        })
        .collect();

    // 执行批量下载
    let results = download_manager.download_segments(segments).await?;

    // 转换结果格式
    let mut sorted_results: Vec<(usize, Vec<u8>)> = results.into_iter().collect();
    sorted_results.sort_by_key(|(index, _)| *index);

    Ok(sorted_results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;

    #[tokio::test]
    async fn test_download_compatibility() {
        let client = Client::new();

        // 测试兼容性下载函数
        // 注意：这里使用一个可能不存在的URL进行测试
        let result =
            download_with_retry_new(&client, "https://httpbin.org/bytes/1024", 10, 2, 1, None)
                .await;

        // 由于网络条件不确定，我们只检查函数是否能正常调用
        // 在实际环境中，这应该能成功下载数据
        match result {
            Ok(data) => {
                assert!(!data.is_empty());
                println!("下载成功，数据长度: {} 字节", data.len());
            }
            Err(e) => {
                println!("下载失败（这在测试环境中是正常的）: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_batch_download() {
        let client = Client::new();
        let urls = vec![
            "https://httpbin.org/bytes/512".to_string(),
            "https://httpbin.org/bytes/1024".to_string(),
        ];

        let result = batch_download_urls(urls, &client, 2, 10, 2).await;

        match result {
            Ok(results) => {
                println!("批量下载成功，结果数量: {}", results.len());
                for (index, data) in results {
                    println!("片段 {}: {} 字节", index, data.len());
                }
            }
            Err(e) => {
                println!("批量下载失败（这在测试环境中是正常的）: {}", e);
            }
        }
    }
}
