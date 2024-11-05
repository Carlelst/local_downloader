use reqwest::Client;
use video_processor::m3u8_parser::parse_m3u8;

#[tokio::test]
async fn test_parse_m3u8_with_key() {
    let client = Client::new();
    let m3u8_content = r#"
        #EXTM3U
        #EXT-X-KEY:METHOD=AES-128,URI="https://example.com/key",IV=0x1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d
        #EXTINF:10.0,
        https://example.com/segment1.ts
        #EXTINF:10.0,
        https://example.com/segment2.ts
    "#;
    let result = parse_m3u8(m3u8_content, &client).await;
    assert!(result.is_ok());

    let (ts_urls, aes_key, iv) = result.unwrap();
    assert_eq!(ts_urls.len(), 2);
    assert!(aes_key.is_some());
    assert!(iv.is_some());
}
