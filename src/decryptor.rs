use aes::Aes128;
use block_modes::{BlockMode, Cbc, block_padding::Pkcs7};
use anyhow::{Result, ensure, Context};

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
    // 确保密钥和 IV 长度为 16 字节
    ensure!(key.len() == 16, "Invalid AES key length: expected 16 bytes, got {}", key.len());
    ensure!(iv.len() == 16, "Invalid AES IV length: expected 16 bytes, got {}", iv.len());

    // 创建 AES-128 CBC 加密对象并解密数据
    Aes128Cbc::new_from_slices(key, iv)
        .context("Failed to create AES cipher")?
        .decrypt_vec(data)
        .context("Decryption failed")
}