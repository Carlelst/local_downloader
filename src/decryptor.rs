use aes::Aes128;
use block_modes::{BlockMode, Cbc};
use block_modes::block_padding::Pkcs7;
use anyhow::{Context, Result};

type Aes128Cbc = Cbc<Aes128, Pkcs7>;

/// 使用 AES-128-CBC 算法解密视频片段数据
///
/// # 参数
/// - `data`: 待解密的字节数据
/// - `key`: 16 字节的 AES 密钥
/// - `iv`: 16 字节的初始化向量
///
/// # 返回
/// 解密后的字节向量，或解密失败的错误信息
pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 验证 key 和 iv 的长度是否为 16 字节
    if key.len() != 16 {
        anyhow::bail!("Invalid AES key length: expected 16 bytes, got {}", key.len());
    }
    if iv.len() != 16 {
        anyhow::bail!("Invalid AES IV length: expected 16 bytes, got {}", iv.len());
    }

    // 创建 AES-128 CBC 加密对象并进行解密
    let cipher = Aes128Cbc::new_from_slices(key, iv)
        .context("Failed to create AES cipher with provided key and IV")?;
    
    let decrypted_data = cipher.decrypt_vec(data)
        .context("Decryption failed for the provided data")?;
    
    Ok(decrypted_data)
}
