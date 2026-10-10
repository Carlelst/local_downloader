use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use log::debug;
use std::time::Instant;

use crate::domain::entities::{EncryptionInfo, EncryptionMethod};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// AES-128-CBC解密功能（向后兼容函数）
///
/// # 参数
/// - `data`: 待解密的数据
/// - `key`: 16字节的AES-128密钥
/// - `iv`: 16字节的初始化向量
///
/// # 返回
/// 成功时返回解密后的数据
pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    decrypt_segment_with_method(data, key, iv, &EncryptionMethod::Aes128)
}

/// 使用指定加密方法解密片段数据
///
/// # 参数
/// - `data`: 加密的数据
/// - `key`: AES 密钥（AES-128为16字节，AES-256为32字节）
/// - `iv`: 初始化向量（16字节）
/// - `method`: 加密方法
///
/// # 返回
/// 成功时返回解密后的数据
pub fn decrypt_segment_with_method(
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    method: &EncryptionMethod,
) -> Result<Vec<u8>> {
    match method {
        EncryptionMethod::None => {
            debug!("无加密，直接返回原始数据");
            Ok(data.to_vec())
        }
        EncryptionMethod::Aes128 => decrypt_aes128_segment(data, key, iv),
        EncryptionMethod::Aes256 => decrypt_aes256_segment(data, key, iv),
        EncryptionMethod::SampleAes => decrypt_sample_aes_segment(data, key, iv),
    }
}

/// 使用加密信息解密片段数据
///
/// # 参数
/// - `data`: 加密的数据
/// - `encryption`: 加密信息
///
/// # 返回
/// 成功时返回解密后的数据
pub fn decrypt_segment_with_info(data: &[u8], encryption: &EncryptionInfo) -> Result<Vec<u8>> {
    let key = encryption
        .key_data
        .as_ref()
        .ok_or_else(|| anyhow!("缺少加密密钥数据"))?;

    let iv = encryption
        .iv
        .as_ref()
        .ok_or_else(|| anyhow!("缺少初始化向量"))?;

    decrypt_segment_with_method(data, key, iv, &encryption.method)
}

/// 解密 AES-128 加密的片段数据
fn decrypt_aes128_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 验证密钥和IV长度
    if key.len() != 16 {
        return Err(anyhow!("AES-128需要16字节密钥，但提供了{}字节", key.len()));
    }
    if iv.len() != 16 {
        return Err(anyhow!("AES-128需要16字节IV，但提供了{}字节", iv.len()));
    }

    if data.is_empty() {
        debug!("数据为空，直接返回空结果");
        return Ok(Vec::new());
    }

    let start_time = Instant::now();
    let total_bytes = data.len();

    // 统一走单线程解密：AES-NI 硬件加速下 CBC 解密速度为 GB/s 级，
    // 2-4MB 的 HLS 分片解密耗时 <5ms，并行分块毫无收益。
    // （历史教训：旧的 >2MB 并行分块实现在中间块上做 PKCS7 校验必然失败，
    //   失败后回退返回密文，导致大分片"解密"输出仍是密文）
    let result = simple_decrypt(data, key, iv)?;
    let elapsed = start_time.elapsed();
    debug!("解密 {} 字节完成，用时: {:.2?}", total_bytes, elapsed);
    Ok(result)
}

/// 单线程 AES-128-CBC 解密（含 PKCS7 去填充）
fn simple_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 创建解密器
    let cipher = Aes128CbcDec::new_from_slices(key, iv).context("创建解密器失败")?;

    // 解密数据（PKCS7 填充校验失败 = key/IV 错误或数据被截断/污染，必须报错，
    // 绝不能把密文当明文返回——那会让下游把垃圾数据写进输出文件）
    cipher.decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|e| {
        anyhow!(
            "AES-128-CBC 解密失败（padding 校验不通过，数据损坏或 key/IV 错误）: {:?}",
            e
        )
    })
}

/// 解密 AES-256 加密的片段数据
fn decrypt_aes256_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 验证密钥和IV长度
    if key.len() != 32 {
        return Err(anyhow!("AES-256需要32字节密钥，但提供了{}字节", key.len()));
    }
    if iv.len() != 16 {
        return Err(anyhow!("AES-256需要16字节IV，但提供了{}字节", iv.len()));
    }

    if data.is_empty() {
        debug!("数据为空，直接返回空结果");
        return Ok(Vec::new());
    }

    let start_time = Instant::now();
    let total_bytes = data.len();

    let result = simple_decrypt_aes256(data, key, iv)?;

    let elapsed = start_time.elapsed();
    debug!(
        "AES-256解密 {} 字节完成，用时: {:.2?}",
        total_bytes, elapsed
    );
    Ok(result)
}

/// 单线程 AES-256-CBC 解密（含 PKCS7 去填充）
fn simple_decrypt_aes256(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 创建解密器
    let cipher = Aes256CbcDec::new_from_slices(key, iv).context("创建AES-256解密器失败")?;

    // 同 AES-128：padding 校验失败必须报错，不能把密文当明文返回
    cipher.decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|e| {
        anyhow!(
            "AES-256-CBC 解密失败（padding 校验不通过，数据损坏或 key/IV 错误）: {:?}",
            e
        )
    })
}

/// 解密 SAMPLE-AES 加密的片段数据
/// SAMPLE-AES 只加密音频和视频的样本数据，不加密整个流
fn decrypt_sample_aes_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 验证密钥和IV长度
    if key.len() != 16 {
        return Err(anyhow!(
            "SAMPLE-AES需要16字节密钥，但提供了{}字节",
            key.len()
        ));
    }
    if iv.len() != 16 {
        return Err(anyhow!("SAMPLE-AES需要16字节IV，但提供了{}字节", iv.len()));
    }

    if data.is_empty() {
        debug!("数据为空，直接返回空结果");
        return Ok(Vec::new());
    }

    debug!("处理SAMPLE-AES解密，数据长度: {} 字节", data.len());

    // SAMPLE-AES的实现比较复杂，因为它需要解析TS包结构
    // 这里提供一个简化的实现，实际应用中可能需要更复杂的TS包解析

    // 对于简化实现，我们假设整个数据都是加密的样本数据
    // 在实际实现中，需要解析TS包头，找到加密的样本数据部分

    let start_time = Instant::now();

    // 使用AES-128 CBC模式解密（SAMPLE-AES基于AES-128）
    let result = simple_decrypt(data, key, iv)?;

    let elapsed = start_time.elapsed();
    debug!(
        "SAMPLE-AES解密 {} 字节完成，用时: {:.2?}",
        data.len(),
        elapsed
    );

    Ok(result)
}

/// 检测密钥格式并返回适当的密钥长度
pub fn detect_key_format(key_data: &[u8]) -> Result<EncryptionMethod> {
    match key_data.len() {
        16 => Ok(EncryptionMethod::Aes128),
        32 => Ok(EncryptionMethod::Aes256),
        _ => Err(anyhow!(
            "不支持的密钥长度: {} 字节。支持的长度: 16字节(AES-128), 32字节(AES-256)",
            key_data.len()
        )),
    }
}

/// 验证加密参数的有效性
pub fn validate_encryption_params(method: &EncryptionMethod, key: &[u8], iv: &[u8]) -> Result<()> {
    // 验证密钥长度
    match method {
        EncryptionMethod::None => {
            // 无加密时不需要密钥和IV
            Ok(())
        }
        EncryptionMethod::Aes128 | EncryptionMethod::SampleAes => {
            // 验证IV长度
            if iv.len() != 16 {
                return Err(anyhow!("IV长度必须为16字节，但提供了{}字节", iv.len()));
            }
            // 验证密钥长度
            if key.len() != 16 {
                Err(anyhow!(
                    "AES-128/SAMPLE-AES需要16字节密钥，但提供了{}字节",
                    key.len()
                ))
            } else {
                Ok(())
            }
        }
        EncryptionMethod::Aes256 => {
            // 验证IV长度
            if iv.len() != 16 {
                return Err(anyhow!("IV长度必须为16字节，但提供了{}字节", iv.len()));
            }
            // 验证密钥长度
            if key.len() != 32 {
                Err(anyhow!("AES-256需要32字节密钥，但提供了{}字节", key.len()))
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_segment_empty_data() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let data = vec![];

        let result = decrypt_segment(&data, &key, &iv).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_decrypt_segment_invalid_key_size() {
        let key = [0u8; 8]; // 无效的密钥大小
        let iv = [0u8; 16];
        let data = vec![0u8; 16];

        let result = decrypt_segment(&data, &key, &iv);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_segment_invalid_iv_size() {
        let key = [0u8; 16];
        let iv = [0u8; 8]; // 无效的IV大小
        let data = vec![0u8; 16];

        let result = decrypt_segment(&data, &key, &iv);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_segment_with_method_aes128() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let data = vec![];

        let result =
            decrypt_segment_with_method(&data, &key, &iv, &EncryptionMethod::Aes128).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_decrypt_segment_with_method_aes256() {
        let key = [0u8; 32];
        let iv = [0u8; 16];
        let data = vec![];

        let result =
            decrypt_segment_with_method(&data, &key, &iv, &EncryptionMethod::Aes256).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_decrypt_segment_with_method_none() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let data = vec![1, 2, 3, 4, 5];

        let result =
            decrypt_segment_with_method(&data, &key, &iv, &EncryptionMethod::None).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_decrypt_segment_with_method_sample_aes() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let data = vec![];

        let result =
            decrypt_segment_with_method(&data, &key, &iv, &EncryptionMethod::SampleAes).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_decrypt_aes256_invalid_key_size() {
        let key = [0u8; 16]; // 无效的密钥大小（AES-256需要32字节）
        let iv = [0u8; 16];
        let data = vec![0u8; 16];

        let result = decrypt_aes256_segment(&data, &key, &iv);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_key_format() {
        let key_128 = [0u8; 16];
        let key_256 = [0u8; 32];
        let key_invalid = [0u8; 24];

        assert_eq!(
            detect_key_format(&key_128).unwrap(),
            EncryptionMethod::Aes128
        );
        assert_eq!(
            detect_key_format(&key_256).unwrap(),
            EncryptionMethod::Aes256
        );
        assert!(detect_key_format(&key_invalid).is_err());
    }

    #[test]
    fn test_validate_encryption_params() {
        let key_128 = [0u8; 16];
        let key_256 = [0u8; 32];
        let iv = [0u8; 16];
        let invalid_iv = [0u8; 8];

        // 有效参数
        assert!(validate_encryption_params(&EncryptionMethod::Aes128, &key_128, &iv).is_ok());
        assert!(validate_encryption_params(&EncryptionMethod::Aes256, &key_256, &iv).is_ok());
        assert!(validate_encryption_params(&EncryptionMethod::SampleAes, &key_128, &iv).is_ok());
        assert!(validate_encryption_params(&EncryptionMethod::None, &[], &[]).is_ok());

        // 无效参数
        assert!(validate_encryption_params(&EncryptionMethod::Aes128, &key_256, &iv).is_err()); // 错误的密钥长度
        assert!(validate_encryption_params(&EncryptionMethod::Aes256, &key_128, &iv).is_err()); // 错误的密钥长度
        assert!(
            validate_encryption_params(&EncryptionMethod::Aes128, &key_128, &invalid_iv).is_err()
        ); // 错误的IV长度
    }

    #[test]
    fn test_decrypt_segment_with_info() {
        let encryption_info = EncryptionInfo {
            method: EncryptionMethod::None,
            key_url: None,
            key_data: Some(vec![0u8; 16]),
            iv: Some(vec![0u8; 16]),
        };

        let data = vec![1, 2, 3, 4, 5];
        let result = decrypt_segment_with_info(&data, &encryption_info).unwrap();
        assert_eq!(result, data); // 无加密应该返回原始数据
    }

    #[test]
    fn test_decrypt_segment_with_info_missing_key() {
        let encryption_info = EncryptionInfo {
            method: EncryptionMethod::Aes128,
            key_url: None,
            key_data: None, // 缺少密钥
            iv: Some(vec![0u8; 16]),
        };

        let data = vec![1, 2, 3, 4, 5];
        let result = decrypt_segment_with_info(&data, &encryption_info);
        assert!(result.is_err());
    }
}
