use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use rayon::prelude::*;
use std::time::Instant;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const CHUNK_SIZE: usize = 1024 * 1024; // 1MB per thread

/// AES-128-CBC解密功能
///
/// # 参数
/// - `data`: 待解密的数据
/// - `key`: 16字节的AES-128密钥
/// - `iv`: 16字节的初始化向量
///
/// # 返回
/// 成功时返回解密后的数据
pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 验证密钥和IV长度
    if key.len() != 16 {
        return Err(anyhow!("AES-128需要16字节密钥，但提供了{}字节", key.len()));
    }
    if iv.len() != 16 {
        return Err(anyhow!("AES-128需要16字节IV，但提供了{}字节", iv.len()));
    }

    let start_time = Instant::now();
    let total_bytes = data.len();

    // 按块大小划分数据
    if total_bytes > CHUNK_SIZE * 2 {
        // 对大数据使用并行处理
        debug!("使用并行解密处理 {} 字节的数据", total_bytes);
        let result = parallel_decrypt(data, key, iv)?;
        let elapsed = start_time.elapsed();
        debug!(
            "并行解密 {} 字节完成，用时: {:.2?}，速度: {:.2} MB/s",
            total_bytes,
            elapsed,
            (total_bytes as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64()
        );
        Ok(result)
    } else {
        // 对小数据使用单线程处理
        let result = simple_decrypt(data, key, iv)?;
        let elapsed = start_time.elapsed();
        debug!("单线程解密 {} 字节完成，用时: {:.2?}", total_bytes, elapsed);
        Ok(result)
    }
}

/// 简单的单线程解密
fn simple_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 创建解密器
    let cipher = Aes128CbcDec::new_from_slices(key, iv).context("创建解密器失败")?;

    // 解密数据
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .context("解密数据失败")
}

/// 使用rayon并行处理大型数据
fn parallel_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 确保数据大小是16字节的倍数（AES块大小）
    if data.len() % 16 != 0 {
        warn!("数据长度不是16字节的倍数，这可能影响解密结果");
    }

    // 计算块数（以CHUNK_SIZE为单位）
    let chunk_count = data.len().div_ceil(CHUNK_SIZE);
    debug!("将数据分成 {} 个块进行并行解密", chunk_count);

    // 对于CBC模式，每个块需要前一个块的最后16字节作为IV
    // 所以我们需要稍微重叠分块
    let chunks: Vec<_> = (0..chunk_count)
        .map(|i| {
            let start = if i == 0 { 0 } else { i * CHUNK_SIZE - 16 };
            let end = std::cmp::min((i + 1) * CHUNK_SIZE, data.len());
            (i, &data[start..end])
        })
        .collect();

    // 并行解密每个块
    let mut decrypted_chunks: Vec<_> = chunks
        .par_iter() // 使用并行迭代器
        .map(|(i, chunk)| {
            let chunk_iv = if *i == 0 {
                // 第一块使用原始IV
                iv.to_vec()
            } else {
                // 后续块使用前一个密文块的最后16字节作为IV
                let prev_block_end = *i * CHUNK_SIZE - 16;
                data[prev_block_end..prev_block_end + 16].to_vec()
            };

            // 创建解密器
            match Aes128CbcDec::new_from_slices(key, &chunk_iv) {
                Ok(cipher) => {
                    // 解密当前块
                    match cipher.decrypt_padded_vec_mut::<Pkcs7>(if *i == 0 {
                        chunk
                    } else {
                        &chunk[16..]
                    }) {
                        Ok(decrypted) => Ok((*i, decrypted)),
                        Err(e) => Err(anyhow!("解密块 {} 失败: {:?}", i, e)),
                    }
                }
                Err(e) => Err(anyhow!("为块 {} 创建解密器失败: {:?}", i, e)),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    // 按顺序合并解密后的块
    decrypted_chunks.sort_by_key(|(i, _)| *i);
    let mut result = Vec::with_capacity(data.len());
    for (_, chunk) in decrypted_chunks {
        result.extend_from_slice(&chunk);
    }

    Ok(result)
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
}
