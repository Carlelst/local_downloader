/// 片段处理器基础设施实现
/// 提供增强的加密处理支持
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

use crate::decryptor::{decrypt_segment_with_info, validate_encryption_params};
use crate::domain::entities::{EncryptionInfo, EncryptionMethod, Segment, SegmentMetadata};
use crate::domain::services::{ProcessedSegment, SegmentProcessor};
use crate::infrastructure::{
    ConcurrencyConfig, MemoryPoolManager, SmartTaskScheduler, Task, TaskMetadata, TaskPriority,
    TaskType,
};

/// 默认片段处理器实现
pub struct DefaultSegmentProcessor {
    /// 是否启用并行处理
    enable_parallel_processing: bool,
    /// 处理超时时间
    processing_timeout: Duration,
    /// 是否验证片段完整性
    validate_integrity: bool,
    /// 智能任务调度器
    task_scheduler: Arc<SmartTaskScheduler>,
    /// 内存池管理器
    memory_pool: Arc<tokio::sync::Mutex<MemoryPoolManager>>,
}

impl DefaultSegmentProcessor {
    pub fn new() -> Self {
        let scheduler_config = ConcurrencyConfig {
            max_concurrent_tasks: num_cpus::get(),
            min_concurrent_tasks: 1,
            max_queue_length: 100,
            backpressure_threshold: 0.8,
            adjustment_interval: Duration::from_secs(5),
            memory_threshold: 512 * 1024 * 1024, // 512MB
            cpu_threshold: 0.7,
        };

        Self {
            enable_parallel_processing: true,
            processing_timeout: Duration::from_secs(30),
            validate_integrity: true,
            task_scheduler: Arc::new(SmartTaskScheduler::new(scheduler_config)),
            memory_pool: Arc::new(tokio::sync::Mutex::new(MemoryPoolManager::new())),
        }
    }

    pub fn with_config(
        enable_parallel_processing: bool,
        processing_timeout: Duration,
        validate_integrity: bool,
    ) -> Self {
        let scheduler_config = ConcurrencyConfig {
            max_concurrent_tasks: if enable_parallel_processing {
                num_cpus::get()
            } else {
                1
            },
            min_concurrent_tasks: 1,
            max_queue_length: 100,
            backpressure_threshold: 0.8,
            adjustment_interval: Duration::from_secs(5),
            memory_threshold: 512 * 1024 * 1024, // 512MB
            cpu_threshold: 0.7,
        };

        Self {
            enable_parallel_processing,
            processing_timeout,
            validate_integrity,
            task_scheduler: Arc::new(SmartTaskScheduler::new(scheduler_config)),
            memory_pool: Arc::new(tokio::sync::Mutex::new(MemoryPoolManager::new())),
        }
    }

    /// 验证加密参数
    fn validate_encryption(&self, encryption: &EncryptionInfo) -> Result<()> {
        let key = encryption
            .key_data
            .as_ref()
            .ok_or_else(|| anyhow!("缺少加密密钥数据"))?;

        let iv = encryption
            .iv
            .as_ref()
            .ok_or_else(|| anyhow!("缺少初始化向量"))?;

        validate_encryption_params(&encryption.method, key, iv)
    }

    /// 检测数据格式
    fn detect_data_format(&self, data: &[u8]) -> DataFormat {
        if data.len() < 4 {
            return DataFormat::Unknown;
        }

        // 检测TS包格式
        if data[0] == 0x47 {
            return DataFormat::TransportStream;
        }

        // 检测其他格式
        if data.starts_with(b"ftyp") || (data.len() >= 8 && data[4..8] == *b"ftyp") {
            return DataFormat::Mp4;
        }

        if data.starts_with(b"\x1A\x45\xDF\xA3") {
            return DataFormat::Webm;
        }

        DataFormat::Unknown
    }

    /// 处理TS格式的片段
    async fn process_ts_segment(
        &self,
        data: &[u8],
        encryption: &Option<EncryptionInfo>,
    ) -> Result<Vec<u8>> {
        match encryption {
            Some(enc) if enc.method != EncryptionMethod::None => {
                debug!("处理加密的TS片段，加密方法: {:?}", enc.method);

                // 验证加密参数
                self.validate_encryption(enc)?;

                // 对于SAMPLE-AES，需要特殊处理
                if enc.method == EncryptionMethod::SampleAes {
                    self.process_sample_aes_ts(data, enc).await
                } else {
                    // 对于AES-128和AES-256，整个片段都是加密的
                    decrypt_segment_with_info(data, enc)
                }
            }
            _ => {
                debug!("处理未加密的TS片段");
                Ok(data.to_vec())
            }
        }
    }

    /// 处理SAMPLE-AES加密的TS片段
    async fn process_sample_aes_ts(
        &self,
        data: &[u8],
        encryption: &EncryptionInfo,
    ) -> Result<Vec<u8>> {
        debug!("处理SAMPLE-AES加密的TS片段，数据长度: {} 字节", data.len());

        // SAMPLE-AES只加密音频和视频的样本数据，不加密TS包头
        // 这里提供一个简化的实现

        let mut result = Vec::with_capacity(data.len());
        let mut pos = 0;

        while pos < data.len() {
            // TS包大小通常是188字节
            let packet_size = std::cmp::min(188, data.len() - pos);
            let packet = &data[pos..pos + packet_size];

            if packet.len() >= 4 && packet[0] == 0x47 {
                // 这是一个有效的TS包
                let processed_packet = self.process_ts_packet(packet, encryption).await?;
                result.extend_from_slice(&processed_packet);
            } else {
                // 不是有效的TS包，直接复制
                result.extend_from_slice(packet);
            }

            pos += packet_size;
        }

        Ok(result)
    }

    /// 处理单个TS包
    async fn process_ts_packet(
        &self,
        packet: &[u8],
        encryption: &EncryptionInfo,
    ) -> Result<Vec<u8>> {
        if packet.len() < 4 {
            return Ok(packet.to_vec());
        }

        // 解析TS包头
        let sync_byte = packet[0];
        if sync_byte != 0x47 {
            return Ok(packet.to_vec());
        }

        let _transport_error_indicator = (packet[1] & 0x80) != 0;
        let _payload_unit_start_indicator = (packet[1] & 0x40) != 0;
        let _transport_priority = (packet[1] & 0x20) != 0;
        let pid = ((packet[1] as u16 & 0x1F) << 8) | packet[2] as u16;
        let transport_scrambling_control = (packet[3] & 0xC0) >> 6;
        let adaptation_field_control = (packet[3] & 0x30) >> 4;
        let _continuity_counter = packet[3] & 0x0F;

        // 如果包被加密（scrambling control != 0），需要解密
        if transport_scrambling_control != 0 {
            debug!(
                "发现加密的TS包，PID: {}, 加密控制: {}",
                pid, transport_scrambling_control
            );

            // 对于SAMPLE-AES，只解密payload部分
            let header_size = 4; // 基本TS头大小
            let mut adaptation_field_size = 0;

            if adaptation_field_control == 2 || adaptation_field_control == 3 {
                // 包含adaptation field
                if packet.len() > 4 {
                    adaptation_field_size = packet[4] as usize + 1;
                }
            }

            let payload_start = header_size + adaptation_field_size;
            if payload_start < packet.len() {
                let mut result = packet.to_vec();
                let payload = &packet[payload_start..];

                if !payload.is_empty() {
                    let decrypted_payload = decrypt_segment_with_info(payload, encryption)?;
                    result[payload_start..].copy_from_slice(
                        &decrypted_payload[..payload.len().min(decrypted_payload.len())],
                    );
                }

                Ok(result)
            } else {
                Ok(packet.to_vec())
            }
        } else {
            // 未加密的包，直接返回
            Ok(packet.to_vec())
        }
    }

    /// 验证处理后的片段
    fn validate_processed_segment(&self, segment: &ProcessedSegment) -> Result<()> {
        if !self.validate_integrity {
            return Ok(());
        }

        // 基本验证
        if segment.data.is_empty() {
            return Err(anyhow!("处理后的片段数据为空"));
        }

        // 根据数据格式进行特定验证
        match self.detect_data_format(&segment.data) {
            DataFormat::TransportStream => self.validate_ts_format(&segment.data),
            DataFormat::Mp4 => self.validate_mp4_format(&segment.data),
            _ => {
                debug!("未知数据格式，跳过格式验证");
                Ok(())
            }
        }
    }

    /// 验证TS格式
    fn validate_ts_format(&self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Err(anyhow!("TS数据为空"));
        }

        // 检查是否以同步字节开始
        if data[0] != 0x47 {
            return Err(anyhow!("TS数据不以同步字节(0x47)开始"));
        }

        // 检查数据长度是否是188的倍数（标准TS包大小）
        if data.len() % 188 != 0 {
            warn!(
                "TS数据长度({})不是188的倍数，可能不是标准TS格式",
                data.len()
            );
        }

        debug!("TS格式验证通过，包含 {} 个TS包", data.len() / 188);
        Ok(())
    }

    /// 验证MP4格式
    fn validate_mp4_format(&self, data: &[u8]) -> Result<()> {
        if data.len() < 8 {
            return Err(anyhow!("MP4数据太短"));
        }

        // 检查ftyp box
        let has_ftyp = data.starts_with(b"ftyp") || (data.len() >= 8 && &data[4..8] == b"ftyp");

        if !has_ftyp {
            return Err(anyhow!("MP4数据缺少ftyp box"));
        }

        debug!("MP4格式验证通过");
        Ok(())
    }
}

impl Clone for DefaultSegmentProcessor {
    fn clone(&self) -> Self {
        Self {
            enable_parallel_processing: self.enable_parallel_processing,
            processing_timeout: self.processing_timeout,
            validate_integrity: self.validate_integrity,
            task_scheduler: Arc::clone(&self.task_scheduler),
            memory_pool: Arc::clone(&self.memory_pool),
        }
    }
}

impl Default for DefaultSegmentProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SegmentProcessor for DefaultSegmentProcessor {
    async fn process_segment(&self, segment: &Segment, data: &[u8]) -> Result<ProcessedSegment> {
        let start_time = SystemTime::now();

        debug!(
            "开始处理片段 {}, 数据长度: {} 字节",
            segment.index,
            data.len()
        );

        // 检测数据格式
        let data_format = self.detect_data_format(data);
        debug!("检测到数据格式: {:?}", data_format);

        // 根据格式和加密信息处理数据
        let processed_data = match data_format {
            DataFormat::TransportStream => {
                self.process_ts_segment(data, &segment.encryption).await?
            }
            _ => {
                // 对于其他格式，使用通用处理
                match &segment.encryption {
                    Some(enc) if enc.method != EncryptionMethod::None => {
                        self.validate_encryption(enc)?;
                        decrypt_segment_with_info(data, enc)?
                    }
                    _ => data.to_vec(),
                }
            }
        };

        let processing_time = start_time.elapsed().unwrap_or(Duration::ZERO);

        let processed_segment = ProcessedSegment {
            index: segment.index,
            data: processed_data,
            metadata: segment.metadata.clone(),
            processing_time,
            is_encrypted: segment.encryption.is_some()
                && segment.encryption.as_ref().unwrap().method != EncryptionMethod::None,
            is_valid: true, // 将在验证后更新
        };

        // 验证处理结果
        match self.validate_processed_segment(&processed_segment) {
            Ok(_) => {
                info!(
                    "片段 {} 处理完成，用时: {:?}",
                    segment.index, processing_time
                );
                Ok(processed_segment)
            }
            Err(e) => {
                warn!("片段 {} 验证失败: {}", segment.index, e);
                let mut invalid_segment = processed_segment;
                invalid_segment.is_valid = false;
                Ok(invalid_segment)
            }
        }
    }

    async fn process_segments(
        &self,
        segments: std::collections::HashMap<usize, Vec<u8>>,
    ) -> Result<Vec<ProcessedSegment>> {
        if !self.enable_parallel_processing || segments.len() <= 1 {
            // 串行处理
            let mut results = Vec::new();

            for (index, data) in segments {
                // 创建临时片段对象
                let segment = Segment {
                    index,
                    url: format!("segment_{}", index),
                    duration: 10.0, // 默认持续时间
                    size: Some(data.len() as u64),
                    encryption: None, // 这里需要从实际上下文获取
                    metadata: SegmentMetadata::default(),
                };

                let processed = self.process_segment(&segment, &data).await?;
                results.push(processed);
            }

            // 按索引排序
            results.sort_by_key(|s| s.index);
            return Ok(results);
        }

        // 启动任务调度器
        self.task_scheduler.start().await?;

        // 启动内存池清理
        {
            let pool = self.memory_pool.lock().await;
            pool.start_cleanup().await?;
        }

        let results = Arc::new(RwLock::new(Vec::new()));
        let _task_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        let expected_count = segments.len();

        for (index, data) in segments {
            let task_id = format!("process_segment_{}", index);

            // 根据数据大小确定优先级
            let priority = if data.len() > 1024 * 1024 {
                // 大于1MB
                TaskPriority::High
            } else {
                TaskPriority::Normal
            };

            let metadata = TaskMetadata {
                task_id: task_id.clone(),
                task_type: TaskType::Processing,
                priority,
                estimated_duration: Some(Duration::from_millis(data.len() as u64 / 1000)), // 估计处理时间
                memory_requirement: Some(data.len() * 2), // 估计需要2倍内存
                cpu_requirement: Some(0.3),               // 处理任务CPU需求中等
                created_at: SystemTime::now(),
                deadline: None,
            };

            let processor = self.clone();
            let results_clone = Arc::clone(&results);

            let future = async move {
                // 创建临时片段对象
                let segment = Segment {
                    index,
                    url: format!("segment_{}", index),
                    duration: 10.0, // 默认持续时间
                    size: Some(data.len() as u64),
                    encryption: None, // 这里需要从实际上下文获取
                    metadata: SegmentMetadata::default(),
                };

                match processor.process_segment(&segment, &data).await {
                    Ok(processed) => {
                        let mut results = results_clone.write().await;
                        results.push(processed);
                        Ok(())
                    }
                    Err(e) => {
                        log::error!("处理片段 {} 失败: {}", index, e);
                        Err(e)
                    }
                }
            };

            let task = Task {
                metadata,
                future: Box::pin(future),
            };

            // 提交任务到调度器
            if let Err(e) = self.task_scheduler.submit_task(task).await {
                log::error!("提交处理任务失败: {}", e);
                continue;
            }
        }

        // 等待所有任务完成
        let mut check_interval = tokio::time::interval(Duration::from_millis(100));
        let start_time = std::time::Instant::now();
        let timeout = self.processing_timeout;

        loop {
            check_interval.tick().await;

            let current_results = results.read().await;
            if current_results.len() >= expected_count {
                break;
            }

            if start_time.elapsed() > timeout {
                log::warn!(
                    "处理超时，已完成 {}/{} 个片段",
                    current_results.len(),
                    expected_count
                );
                break;
            }
        }

        // 停止调度器
        let _ = self.task_scheduler.shutdown().await;

        let mut final_results = results.read().await.clone();

        // 按索引排序
        final_results.sort_by_key(|s| s.index);
        Ok(final_results)
    }

    async fn decrypt_if_needed(&self, data: &[u8], encryption: &EncryptionInfo) -> Result<Vec<u8>> {
        if encryption.method == EncryptionMethod::None {
            debug!("无需解密，直接返回原始数据");
            return Ok(data.to_vec());
        }

        self.validate_encryption(encryption)?;
        decrypt_segment_with_info(data, encryption)
    }

    fn validate_segment(&self, segment: &ProcessedSegment) -> Result<()> {
        self.validate_processed_segment(segment)
    }
}

/// 数据格式枚举
#[derive(Debug, Clone, PartialEq)]
enum DataFormat {
    TransportStream, // MPEG-TS
    Mp4,             // MP4/fMP4
    Webm,            // WebM
    Unknown,         // 未知格式
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::EncryptionMethod;

    #[tokio::test]
    async fn test_process_unencrypted_segment() {
        let processor = DefaultSegmentProcessor::new();
        let data = vec![0x47, 0x00, 0x00, 0x10]; // 简单的TS包头

        let segment = Segment {
            index: 0,
            url: "test.ts".to_string(),
            duration: 10.0,
            size: Some(data.len() as u64),
            encryption: None,
            metadata: SegmentMetadata::default(),
        };

        let result = processor.process_segment(&segment, &data).await;
        assert!(result.is_ok());

        let processed = result.unwrap();
        assert_eq!(processed.index, 0);
        assert_eq!(processed.data, data);
        assert!(!processed.is_encrypted);
    }

    #[tokio::test]
    async fn test_decrypt_if_needed_no_encryption() {
        let processor = DefaultSegmentProcessor::new();
        let data = vec![1, 2, 3, 4, 5];

        let encryption = EncryptionInfo {
            method: EncryptionMethod::None,
            key_url: None,
            key_data: Some(vec![0u8; 16]),
            iv: Some(vec![0u8; 16]),
        };

        let result = processor.decrypt_if_needed(&data, &encryption).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn test_detect_data_format() {
        let processor = DefaultSegmentProcessor::new();

        // TS格式
        let ts_data = vec![0x47, 0x00, 0x00, 0x10];
        assert_eq!(
            processor.detect_data_format(&ts_data),
            DataFormat::TransportStream
        );

        // MP4格式
        let mp4_data = vec![0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p'];
        assert_eq!(processor.detect_data_format(&mp4_data), DataFormat::Mp4);

        // 未知格式
        let unknown_data = vec![0x00, 0x01, 0x02, 0x03];
        assert_eq!(
            processor.detect_data_format(&unknown_data),
            DataFormat::Unknown
        );
    }

    #[test]
    fn test_validate_ts_format() {
        let processor = DefaultSegmentProcessor::new();

        // 有效的TS数据
        let valid_ts = vec![0x47; 188]; // 一个TS包
        assert!(processor.validate_ts_format(&valid_ts).is_ok());

        // 无效的TS数据（不以0x47开始）
        let invalid_ts = vec![0x00; 188];
        assert!(processor.validate_ts_format(&invalid_ts).is_err());

        // 空数据
        let empty_data = vec![];
        assert!(processor.validate_ts_format(&empty_data).is_err());
    }

    #[test]
    fn test_validate_encryption_params() {
        let processor = DefaultSegmentProcessor::new();

        // 有效的AES-128加密信息
        let valid_aes128 = EncryptionInfo {
            method: EncryptionMethod::Aes128,
            key_url: None,
            key_data: Some(vec![0u8; 16]),
            iv: Some(vec![0u8; 16]),
        };
        assert!(processor.validate_encryption(&valid_aes128).is_ok());

        // 无效的加密信息（缺少密钥）
        let invalid_encryption = EncryptionInfo {
            method: EncryptionMethod::Aes128,
            key_url: None,
            key_data: None,
            iv: Some(vec![0u8; 16]),
        };
        assert!(processor.validate_encryption(&invalid_encryption).is_err());
    }
}
