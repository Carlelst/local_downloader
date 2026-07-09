//! Cloudflare 旁路下载器
//!
//! 当目标域名使用 Cloudflare 防护时（常规 reqwest 客户端 TLS 指纹不通过），
//! 通过 spawn Python + curl-cffi 子进程来下载 M3U8 流。
//!
//! curl-cffi 模拟真实 Chrome 的 TLS 指纹（JA3/JA4）、HTTP/2 设置、header 顺序，
//! 完整下载后再用 ffmpeg 转封装为 MP4。

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// 需要走 Cloudflare 旁路的域名列表（小写匹配）
const CF_BYPASS_DOMAINS: &[&str] = &[
    "surrit.com",
    "fourhoi.com",
];

/// Cloudflare 旁路下载器
pub struct CloudflareBypassDownloader {
    python_path: PathBuf,
    script_path: PathBuf,
}

impl CloudflareBypassDownloader {
    pub fn new(project_root: &Path) -> Self {
        // 优先用 miniconda3 里的 Python（含 curl-cffi）
        let home = std::env::var("HOME").unwrap_or_default();
        let python_path = PathBuf::from(format!("{}/miniconda3/bin/python3", home));
        let script_path = project_root.join("scripts/curl_cffi_downloader.py");

        Self { python_path, script_path }
    }

    /// 判断 URL 是否需要走 Cloudflare 旁路
    pub fn needs_bypass(url: &str) -> bool {
        let url_lower = url.to_lowercase();
        CF_BYPASS_DOMAINS.iter().any(|d| url_lower.contains(d))
    }

    /// 调用 Python 脚本下载 M3U8
    ///
    /// - `m3u8_url`: M3U8 地址
    /// - `referer`: 来源页 Referer
    /// - `output`: 最终 MP4 输出路径
    pub async fn download(
        &self,
        m3u8_url: &str,
        referer: &str,
        output: &Path,
    ) -> Result<()> {
        if !self.python_path.exists() {
            return Err(anyhow!(
                "Python 未找到: {}（请安装 miniconda3 并 pip install curl-cffi）",
                self.python_path.display()
            ));
        }
        if !self.script_path.exists() {
            return Err(anyhow!("脚本不存在: {}", self.script_path.display()));
        }

        let work_dir = output.with_extension("work");
        let work_dir_str = work_dir.to_string_lossy().to_string();
        let output_str = output.to_string_lossy().to_string();

        let mut cmd = Command::new(&self.python_path);
        cmd.arg(&self.script_path)
            .arg("--url").arg(m3u8_url)
            .arg("--referer").arg(referer)
            .arg("--output").arg(&output_str)
            .arg("--work-dir").arg(&work_dir_str)
            .arg("--concurrent").arg("24")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        log::info!("[CF Bypass] 启动 curl-cffi 子进程...");
        log::info!("[CF Bypass] URL: {}", m3u8_url);
        log::info!("[CF Bypass] Referer: {}", referer);
        log::info!("[CF Bypass] Output: {}", output_str);

        let mut child = cmd.spawn().context("启动 Python 进程失败")?;

        // 实时透传 stdout/stderr
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("{}", line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[py] {}", line);
                }
            });
        }

        let status = child.wait().await.context("等待 Python 进程失败")?;
        if !status.success() {
            return Err(anyhow!(
                "Python 进程退出码: {}（{}）",
                status.code().unwrap_or(-1),
                status
            ));
        }

        if !output.exists() {
            return Err(anyhow!("输出文件未生成: {}", output.display()));
        }
        log::info!("[CF Bypass] ✓ 下载完成: {}", output.display());
        Ok(())
    }
}
