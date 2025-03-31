use aes::Aes128;
use anyhow::{ensure, Context, Result};
use block_modes::{block_padding::Pkcs7, BlockMode, Cbc};
use log::{debug, warn};

pub type Aes128Cbc = Cbc<Aes128, Pkcs7>;

/// 解密指定数据段
///
/// # 参数
/// - `data`: 待解密的数据字节切片
/// - `key`: AES 解密密钥，必须为 16 字节
/// - `iv`: AES 初始化向量，必须为 16 字节
///
/// # 返回
/// 返回解密后的数据，或在失败时返回错误。
pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() {
        debug!("解密器收到空数据，直接返回空结果");
        return Ok(Vec::new());
    }

    ensure!(
        key.len() == 16,
        "无效的 AES 密钥长度: 期望 16 字节, 实际为 {} 字节",
        key.len()
    );
    ensure!(
        iv.len() == 16,
        "无效的 AES IV 长度: 期望 16 字节, 实际为 {} 字节",
        iv.len()
    );

    let start_time = std::time::Instant::now();

    // 创建 AES 解密器实例并尝试解密
    let result = Aes128Cbc::new_from_slices(key, iv)
        .context("创建 AES 解密器失败")?
        .decrypt_vec(data)
        .context("解密数据失败");

    match result {
        Ok(decrypted) => {
            let elapsed = start_time.elapsed();
            debug!("解密 {} 字节数据成功，耗时: {:?}", data.len(), elapsed);
            Ok(decrypted)
        }
        Err(e) => {
            // 在发生错误时提供更详细的诊断信息
            warn!("解密失败: 数据长度: {}，错误: {:?}", data.len(), e);

            // 检查数据是否为PKCS7填充的有效长度（16的倍数）
            if data.len() % 16 != 0 {
                warn!("数据长度 {} 不是 16 的倍数，可能导致解密失败", data.len());
            }

            Err(e)
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
}
