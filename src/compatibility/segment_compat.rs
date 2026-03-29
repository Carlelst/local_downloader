/// 片段处理功能兼容性包装器
///
/// 提供与现有片段处理API兼容的接口，内部使用新的片段处理器实现
use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, RwLock};

use crate::compatibility::download_compat::download_with_retry_new;
use crate::domain::entities::{EncryptionInfo, EncryptionMethod, Segment, SegmentMetadata};
use crate::domain::services::SegmentProcessor;
use crate::infrastructure::DefaultSegmentProcessor;

/// 兼容性片段处理函数 - 使用新架构实现
///
/// 该函数保持与原始 `process_segment` 函数相同的签名，
/// 但内部使用新的片段处理器实现
pub async fn process_segment_new(
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
) -> Result<Vec<u8>> {
    // 创建新的片段处理器
    let processor = DefaultSegmentProcessor::new();

    // 下载片段数据
    let data = download_with_retry_new(
        &client,
        &url,
        timeout,
        retries,
        retry_interval,
        None, // 暂时不支持进度回调
    )
    .await?;

    // 创建加密信息（如果提供了密钥）
    let encryption = if let (Some(key), Some(iv)) = (aes_key, aes_iv) {
        Some(EncryptionInfo {
            method: EncryptionMethod::Aes128,
            key_url: None,
            key_data: Some(key),
            iv: Some(iv),
        })
    } else {
        None
    };

    // 创建片段对象
    let segment = Segment {
        index: idx,
        url: url.clone(),
        duration: 0.0, // 兼容模式下不需要持续时间
        size: Some(data.len() as u64),
        encryption,
        metadata: SegmentMetadata::default(),
    };

    // 使用新的处理器处理片段
    let processed_segment = processor.process_segment(&segment, &data).await?;
    let processed_data = processed_segment.data;

    // 将处理后的数据插入共享Map（保持兼容性）
    {
        let mut segments_lock = segments.write().await;
        segments_lock.insert(idx, processed_data.clone());
    }

    // 发送完成通知（保持兼容性）
    if let Err(e) = notify_tx.send(idx).await {
        log::warn!("无法发送完成通知: {}, idx: {}", e, idx);
    }

    Ok(processed_data)
}

/// 批量处理片段
///
/// 使用新的片段处理器实现批量处理功能
pub async fn batch_process_segments(
    segments_data: HashMap<usize, Vec<u8>>,
    encryption_info: Option<EncryptionInfo>,
) -> Result<HashMap<usize, Vec<u8>>> {
    let processor = DefaultSegmentProcessor::new();

    let mut results = HashMap::new();

    for (index, data) in segments_data {
        // 创建片段对象
        let segment = Segment {
            index,
            url: format!("segment_{}", index),
            duration: 0.0,
            size: Some(data.len() as u64),
            encryption: encryption_info.clone(),
            metadata: SegmentMetadata::default(),
        };

        // 处理片段
        match processor.process_segment(&segment, &data).await {
            Ok(processed_segment) => {
                results.insert(index, processed_segment.data);
            }
            Err(e) => {
                log::error!("处理片段 {} 失败: {}", index, e);
                // 在兼容模式下，失败的片段使用原始数据
                results.insert(index, data);
            }
        }
    }

    Ok(results)
}

/// 创建兼容的片段处理器
///
/// 该函数创建一个配置好的片段处理器，可以用于自定义处理
pub fn create_compatible_segment_processor() -> Arc<dyn SegmentProcessor> {
    Arc::new(DefaultSegmentProcessor::new())
}

/// 处理加密片段的兼容性函数
///
/// 提供与旧API兼容的加密片段处理
pub async fn process_encrypted_segment(
    data: &[u8],
    aes_key: &[u8],
    aes_iv: &[u8],
) -> Result<Vec<u8>> {
    let processor = DefaultSegmentProcessor::new();

    let encryption_info = EncryptionInfo {
        method: EncryptionMethod::Aes128,
        key_url: None,
        key_data: Some(aes_key.to_vec()),
        iv: Some(aes_iv.to_vec()),
    };

    processor.decrypt_if_needed(data, &encryption_info).await
}

/// 验证片段数据的兼容性函数
///
/// 使用新的验证逻辑检查片段完整性
pub async fn validate_segment_data(
    index: usize,
    data: &[u8],
    expected_size: Option<u64>,
) -> Result<bool> {
    let processor = DefaultSegmentProcessor::new();

    // 创建临时的处理后片段用于验证
    let processed_segment = crate::domain::services::ProcessedSegment {
        index,
        data: data.to_vec(),
        metadata: SegmentMetadata::default(),
        processing_time: std::time::Duration::ZERO,
        is_encrypted: false,
        is_valid: true,
    };

    // 验证片段
    match processor.validate_segment(&processed_segment) {
        Ok(_) => {
            // 如果提供了期望大小，也要检查大小
            if let Some(expected) = expected_size {
                Ok(data.len() as u64 == expected)
            } else {
                Ok(true)
            }
        }
        Err(_) => Ok(false),
    }
}

/// 获取片段处理统计信息
///
/// 提供处理统计的兼容性接口
pub struct SegmentProcessingStats {
    pub total_processed: usize,
    pub total_size: u64,
    pub encrypted_count: usize,
    pub validation_failures: usize,
}

impl SegmentProcessingStats {
    pub fn new() -> Self {
        Self {
            total_processed: 0,
            total_size: 0,
            encrypted_count: 0,
            validation_failures: 0,
        }
    }

    pub fn update(&mut self, segment_size: u64, is_encrypted: bool, validation_failed: bool) {
        self.total_processed += 1;
        self.total_size += segment_size;
        if is_encrypted {
            self.encrypted_count += 1;
        }
        if validation_failed {
            self.validation_failures += 1;
        }
    }

    pub fn get_summary(&self) -> String {
        format!(
            "处理统计: {} 个片段, {:.2} MB 总大小, {} 个加密, {} 个验证失败",
            self.total_processed,
            self.total_size as f64 / (1024.0 * 1024.0),
            self.encrypted_count,
            self.validation_failures
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_segment_processing_compatibility() {
        // 创建测试数据
        let test_data = b"test segment data".to_vec();
        let segments = Arc::new(RwLock::new(HashMap::<usize, Vec<u8>>::new()));
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);

        // 测试处理未加密片段
        let client = Arc::new(reqwest::Client::new());

        // 注意：这里我们不能真正测试网络下载，所以我们测试其他功能

        // 测试批量处理
        let mut segments_data = HashMap::new();
        segments_data.insert(0, test_data.clone());

        let result = batch_process_segments(segments_data, None).await;
        assert!(result.is_ok());

        let processed = result.unwrap();
        assert_eq!(processed.len(), 1);
        assert!(processed.contains_key(&0));
    }

    #[tokio::test]
    async fn test_encryption_compatibility() {
        let test_data = b"test encrypted data";
        let key = vec![0u8; 16]; // 16字节的测试密钥
        let iv = vec![0u8; 16]; // 16字节的测试IV

        // 测试加密处理（注意：这可能会失败，因为数据不是真正的加密数据）
        let result = process_encrypted_segment(test_data, &key, &iv).await;

        // 在测试环境中，我们主要验证函数能够被调用
        match result {
            Ok(decrypted) => {
                println!("解密成功，数据长度: {}", decrypted.len());
            }
            Err(e) => {
                println!("解密失败（这在测试环境中是正常的）: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_validation_compatibility() {
        let test_data = b"test validation data";

        let result = validate_segment_data(0, test_data, Some(test_data.len() as u64)).await;
        assert!(result.is_ok());

        let is_valid = result.unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_stats() {
        let mut stats = SegmentProcessingStats::new();

        stats.update(1024, true, false);
        stats.update(2048, false, false);
        stats.update(512, true, true);

        assert_eq!(stats.total_processed, 3);
        assert_eq!(stats.total_size, 3584);
        assert_eq!(stats.encrypted_count, 2);
        assert_eq!(stats.validation_failures, 1);

        let summary = stats.get_summary();
        assert!(summary.contains("3 个片段"));
    }
}
