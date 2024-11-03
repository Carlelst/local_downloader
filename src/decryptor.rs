use aes::Aes128;
use block_modes::{BlockMode, Cbc};
use block_modes::block_padding::Pkcs7;
use anyhow::Result;

type Aes128Cbc = Cbc<Aes128, Pkcs7>;

pub fn decrypt_segment(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes128Cbc::new_from_slices(key, iv)?;
    let decrypted_data = cipher.decrypt_vec(data)?;
    Ok(decrypted_data)
}
