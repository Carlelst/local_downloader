use aes::Aes128;
use block_modes::{BlockMode, Cbc, block_padding::Pkcs7};
use anyhow::{Result, ensure, Context};

pub type Aes128Cbc = Cbc<Aes128, Pkcs7>;

pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    ensure!(key.len() == 16, "Invalid AES key length: expected 16 bytes, got {}", key.len());
    ensure!(iv.len() == 16, "Invalid AES IV length: expected 16 bytes, got {}", iv.len());

    Aes128Cbc::new_from_slices(key, iv).context("Failed to create AES cipher")?
        .decrypt_vec(data).context("Decryption failed")
}
