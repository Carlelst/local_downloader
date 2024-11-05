use reqwest::Client;
use regex::Regex;
use anyhow::Result;

/// 解析 M3U8 文件，提取 ts 文件 URL 列表、AES 密钥和 IV
pub async fn parse_m3u8(m3u8_url: &str, client: &Client) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let response = client.get(m3u8_url).send().await?;
    let m3u8_content = response.text().await?;

    let ts_urls: Vec<String> = m3u8_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    let key_re = Regex::new(r#"EXT-X-KEY:METHOD=AES-128,URI="([^"]+)"(?:,IV=(0x[0-9a-fA-F]+))?"#)?;
    let (key_url, iv) = if let Some(captures) = key_re.captures(&m3u8_content) {
        let key_url = captures.get(1)
            .ok_or_else(|| anyhow::anyhow!("Key URL not found"))?
            .as_str()
            .to_string();

        let iv = if let Some(iv_match) = captures.get(2) {
            let iv_hex = &iv_match.as_str()[2..];
            hex::decode(iv_hex).ok()
        } else {
            Some(vec![0; 16])
        };

        (Some(key_url), iv)
    } else {
        (None, None)
    };

    let aes_key = if let Some(url) = key_url {
        Some(client.get(&url).send().await?.bytes().await?.to_vec())
    } else {
        None
    };

    Ok((ts_urls, aes_key, iv))
}
