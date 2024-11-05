use criterion::{criterion_group, criterion_main, Criterion};
use reqwest::Client;
use tokio::runtime::Runtime;

fn benchmark_download_with_retry(c: &mut Criterion) {
    let client = Client::new();
    let url = "https://httpbin.org/bytes/1024"; // 模拟的下载URL
    let rt = Runtime::new().unwrap();

    c.bench_function("download_with_retry", |b| {
        b.to_async(&rt).iter(|| download_with_retry(&client, url, 3))
    });
}

fn benchmark_ffmpeg_write(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut ffmpeg = start_ffmpeg().await.unwrap();
        let ffmpeg_stdin = ffmpeg.stdin.as_mut().unwrap();

        c.bench_function("ffmpeg_write", |b| {
            b.to_async(&rt).iter(|| async {
                ffmpeg_stdin.write_all(b"test data").await.unwrap();
            })
        });
    });
}

criterion_group!(benches, benchmark_download_with_retry, benchmark_ffmpeg_write);
criterion_main!(benches);
