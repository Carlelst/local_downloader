use reqwest::Client;
use video_processor::downloader::download_with_retry;  // 导入函数

#[tokio::test]
async fn test_download_with_retry_success() {
    let client = Client::new();
    let url = "https://httpbin.org/bytes/1024";
    let result = download_with_retry(&client, url, 3).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1024);
}

#[tokio::test]
async fn test_download_with_retry_failure() {
    let client = Client::new();
    let url = "https://httpbin.org/status/404";
    let result = download_with_retry(&client, url, 3).await;
    assert!(result.is_err());
}
