use aes::Aes128;
use block_modes::{BlockMode, Cbc, block_padding::Pkcs7};
use anyhow::{Result, ensure, Context};

pub type Aes128Cbc = Cbc<Aes128, Pkcs7>;

pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    // 确保 AES 密钥长度为 16 字节
    ensure!(key.len() == 16, "无效的 AES 密钥长度: 期望 16 字节, 实际为 {} 字节", key.len());
    // 确保 AES IV 长度为 16 字节
    ensure!(iv.len() == 16, "无效的 AES IV 长度: 期望 16 字节, 实际为 {} 字节", iv.len());

    // 创建 AES 加密器并进行解密
    Aes128Cbc::new_from_slices(key, iv)
        .context("创建 AES 加密器失败")?
        .decrypt_vec(data)
        .context("解密失败")
}
