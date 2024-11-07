use reqwest::Client;
use regex::Regex;
use anyhow::Result;
use url::Url;

/// 解析 M3U8 文件，兼容处理 TS 文件和 JPEG 文件
pub async fn parse_m3u8(m3u8_url: &str, client: &Client) -> Result<(Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let response = client.get(m3u8_url).send().await?;
    let m3u8_content = response.text().await?;
    
    // 提取 M3U8 URL 的基础路径
    let base_url = Url::parse(m3u8_url)?.join("")?;
    
    let ts_urls: Vec<String> = m3u8_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            if line.starts_with("https") && line.ends_with(".ts") {
                // 如果行以 https 开头且以 .ts 结尾，直接使用
                line.to_string()
            } else if line.ends_with(".jpeg") {
                // 将 .jpeg 文件后缀替换为 .ts 并拼接完整 URL
                let modified_line = line.trim_end_matches(".jpeg").to_string() + ".ts";
                base_url.join(&modified_line)
                    .map(|url| url.to_string())
                    .unwrap_or_else(|e| {
                        eprintln!("Warning: Failed to parse URL segment '{}': {}", modified_line, e);
                        modified_line  // 返回修改后的行用于调试
                    })
            } else if line.ends_with(".ts") {
                // 处理 .ts 文件，拼接完整 URL
                base_url.join(line)
                    .map(|url| url.to_string())
                    .unwrap_or_else(|e| {
                        eprintln!("Warning: Failed to parse URL segment '{}': {}", line, e);
                        line.to_string()  // 返回原始行用于调试
                    })
            } else {
                // 其他类型的资源路径，直接返回原始行
                line.to_string()
            }
        })
        .collect();

    // 如果文件中包含 AES 密钥定义，则解析并返回（保留旧逻辑）
    let key_re = Regex::new(r#"EXT-X-KEY:METHOD=AES-128,URI="([^"]+)"(?:,IV=(0x[0-9a-fA-F]+))?"#)?;
    let (key_url, iv) = if let Some(captures) = key_re.captures(&m3u8_content) {
        let key_url = captures.get(1).map(|m| m.as_str().to_string());
        let iv = captures.get(2).map(|m| hex::decode(&m.as_str()[2..]).ok()).flatten();
        (key_url, iv)
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
