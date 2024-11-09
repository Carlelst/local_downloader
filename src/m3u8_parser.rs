use reqwest::Client;
use regex::Regex;
use anyhow::{Result, Context};
use url::Url;
use log::{debug, info};

/// 解析 M3U8 URL 并返回 ts 片段 URL、AES 密钥及 IV。
/// 
/// # 参数
/// - `m3u8_url`: M3U8 文件的 URL
/// - `client`: HTTP 客户端
/// 
/// # 返回
/// 返回包含 ts 片段 URL 列表、可选的 AES 密钥和 IV。
pub async fn parse_m3u8(m3u8_url: &str, client: &Client) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    debug!("获取 M3U8 内容，URL: {}", m3u8_url);
    let response = client.get(m3u8_url).send().await.context("无法获取 M3U8 内容")?;
    let m3u8_content = response.text().await.context("读取 M3U8 内容失败")?;

    let base_url = Url::parse(m3u8_url)?.join("")?;
    debug!("M3U8 基础 URL: {}", base_url);

    let ts_urls: Vec<String> = m3u8_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| transform_line_to_url(line, &base_url))
        .collect();

    // 解析 AES 密钥和 IV
    let key_re = Regex::new(r#"EXT-X-KEY:METHOD=AES-128,URI=\"([^\"]+)\"(?:,IV=(0x[0-9a-fA-F]+))?"#)?;
    let (key_url, iv) = if let Some(captures) = key_re.captures(&m3u8_content) {
        let key_url = captures.get(1).map(|m| m.as_str().to_string());
        let iv = captures.get(2).map(|m| hex::decode(&m.as_str()[2..]).ok()).flatten();
        debug!("解析到的 AES 密钥 URL: {:?}, IV: {:?}", key_url, iv);
        (key_url, iv)
    } else {
        (None, None)
    };

    let aes_key = if let Some(url) = key_url {
        debug!("从 URL 获取 AES 密钥: {}", url);
        client.get(&url).send().await?.bytes().await?.to_vec().into()
    } else {
        info!("M3U8 内容中未找到 AES 密钥");
        None
    };

    Ok((ts_urls, aes_key, iv))
}

/// 转换相对路径为完整 URL
/// 
/// # 参数
/// - `line`: M3U8 文件中的行
/// - `base_url`: M3U8 文件的基础 URL
/// 
/// # 返回
/// 返回完整的 ts 或 jpeg URL。
fn transform_line_to_url(line: &str, base_url: &Url) -> String {
    if line.contains(".ts") || line.ends_with(".jpeg") {
        return base_url.join(line).map(|url| url.to_string()).unwrap_or_else(|_| line.to_string());
    }
    line.to_string()
}
