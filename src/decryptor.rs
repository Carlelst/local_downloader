use aes::Aes128;
use block_modes::{BlockMode, Cbc, block_padding::Pkcs7};
use anyhow::{Result, ensure, Context};

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
    ensure!(key.len() == 16, "无效的 AES 密钥长度: 期望 16 字节, 实际为 {} 字节", key.len());
    ensure!(iv.len() == 16, "无效的 AES IV 长度: 期望 16 字节, 实际为 {} 字节", iv.len());

    // 创建 AES 解密器实例并尝试解密
    Aes128Cbc::new_from_slices(key, iv)
        .context("创建 AES 解密器失败")?
        .decrypt_vec(data)
        .context("解密数据失败")
}