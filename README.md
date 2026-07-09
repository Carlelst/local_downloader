# 视频处理器 (Video Processor)

一个高性能的 Rust 视频流下载和处理工具，专门用于处理 M3U8 视频流。支持并发下载、AES 解密、硬件加速编码和断点续传功能。

## 🚀 主要特性

- **高并发下载**: 支持多线程并发下载视频片段，自动调整并发数
- **AES 解密**: 自动处理加密的视频流，支持 AES-128-CBC 解密
- **硬件加速**: 支持 GPU 硬件加速编码，提升处理速度
- **断点续传**: 支持下载中断后的断点续传功能
- **自适应策略**: 根据网络状况自动调整下载策略
- **多格式输出**: 支持 MP4、MKV、FLV 等多种视频格式输出
- **实时监控**: 提供下载进度和性能监控
- **🛡️ Cloudflare 旁路**: 通过 curl-cffi 模拟 Chrome TLS 指纹，无需 cf_clearance cookie 即可下载被 CF 防护的视频流（surrit.com、fourhoi.com 等）
- **📝 智能命名**: 自动从 URL 提取 UUID 作为输出文件名（如 `0f6dce72-...mp4`），避免哈希乱码

## 📦 安装要求

### 系统要求

- Rust 1.70+
- FFmpeg (用于视频处理)
- macOS / Linux / Windows

### 依赖安装

**macOS (使用 Homebrew):**

```bash
brew install ffmpeg
```

**Ubuntu/Debian:**

```bash
sudo apt update
sudo apt install ffmpeg
```

**Windows:**
下载 FFmpeg 并添加到系统 PATH 中。

### Python 依赖（Cloudflare 旁路）

如需下载被 Cloudflare 防护的视频源，需安装 curl-cffi：

```bash
# 使用 miniconda3
/Users/lst/miniconda3/bin/pip install curl-cffi
```

## 🛠️ 编译安装

```bash
# 克隆项目
git clone <repository-url>
cd video_processor

# 编译项目
cargo build --release

# 运行开发环境设置
./scripts/dev-setup.sh
```

## ⚙️ 配置文件

创建 `config.yaml` 配置文件：

```yaml
# M3U8 视频流地址列表
m3u8_urls:
  - "https://example.com/playlist.m3u8"
  - "https://example.com/another-playlist.m3u8"

# 下载配置
max_concurrent_downloads: 16 # 最大并发下载数
download_retries: 3 # 下载重试次数
segment_timeout: 20 # 片段下载超时时间(秒)
retry_interval: 2 # 重试间隔(秒)
chunk_size: 2097152 # 下载块大小(字节，2MB)

# 输出配置
output_directory: "./downloads" # 输出目录
video_format: "mp4" # 输出格式 (mp4, mkv, flv)
video_codec: "copy" # 视频编码 (copy, h264, h265)
audio_codec: "aac" # 音频编码 (aac, copy, mp3)

# 系统配置
log_level: "info" # 日志级别 (debug, info, warn, error)
buffer_size: 8388608 # 缓冲区大小(字节，8MB)
validate_segments: false # 是否验证片段
use_hw_accel: true # 启用硬件加速
thread_count: 4 # FFmpeg 线程数
```

## 🚀 使用方法

### 基本使用

```bash
# 使用默认配置运行
cargo run --release

# 或者使用编译后的可执行文件
./target/release/video_processor
```

### 命令行选项

```bash
# 强制重新下载所有片段
cargo run --release -- --force

# 清除下载缓存
cargo run --release -- --clear-cache

# 指定视频比特率
cargo run --release -- --bitrate=2M

# 使用复制模式(不重新编码)
cargo run --release -- --copy

# 禁用硬件加速
cargo run --release -- --no-hw
```

### 编程接口使用

```rust
use video_processor::{load_config, VideoProcessorService, create_default_service_factory};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 加载配置
    let config = load_config()?;

    // 创建服务工厂
    let factory = create_default_service_factory().await?;

    // 获取视频处理服务
    let processor = factory.video_processor_service().await?;

    // 处理 M3U8 视频流
    for url in &config.m3u8_urls {
        let result = processor.process_stream(url, &config.output_directory).await?;
        println!("处理完成: {:?}", result);
    }

    Ok(())
}
```

## 🛡️ Cloudflare 旁路下载（推荐）

针对使用 Cloudflare 防护的视频源（`surrit.com`、`fourhoi.com` 等），传统 reqwest 客户端的 TLS 指纹会被识别为机器人。本项目通过 **curl-cffi 模拟真实 Chrome 浏览器**的 TLS 指纹（JA3/JA4）、HTTP/2 设置和 header 顺序，**无需 cf_clearance cookie** 即可通过验证。

### 工作原理

- 检测到 `surrit.com` / `fourhoi.com` 域名 → 自动走 Python + curl-cffi 子进程
- curl-cffi 在 TLS 握手时返回与 Chrome 一致的指纹
- 完整下载所有 TS 片段后再用 ffmpeg 转封装为 MP4
- 其他域名继续走原有 Rust reqwest 流程

### 安装 curl-cffi

```bash
# 使用 miniconda3 或系统 Python
/Users/lst/miniconda3/bin/pip install curl-cffi
```

**验证安装**：

```bash
/Users/lst/miniconda3/bin/python3 -c "from curl_cffi import requests; print('✓ curl_cffi OK')"
```

### 使用方式

普通使用方式 **完全一致**，无需额外参数：

```bash
./target/release/video_processor
```

### 配置文件示例

```yaml
# M3U8 视频流地址列表
m3u8_urls:
  - https://surrit.com/382917dd-2d10-4e99-95fd-4f75f82d7d76/720p/video.m3u8

# Referer 映射（按域名匹配）
referers:
  surrit.com: "https://missav.ws/cn/mird-285-uncensored-leak"

# 输出配置
output_directory: "./downloads"
video_format: "mp4"
video_codec: "copy"
```

### 输出示例

```
[2026-07-09 19:24:30 INFO] [CF Bypass] 启动 curl-cffi 子进程...
[2026-07-09 19:24:30 INFO] [CF Bypass] URL: https://surrit.com/0f6dce72-78a0-4397-aec2-44066a122831/720p/video.m3u8
[2026-07-09 19:24:30 INFO] [CF Bypass] Referer: https://missav.ws/cn/mird-285-uncensored-leak
[2026-07-09 19:24:30 INFO] [CF Bypass] Output: ./downloads/0f6dce72-78a0-4397-aec2-44066a122831.mp4
[2026-07-09 19:27:41 INFO] [py] [curl_cffi_dl] ✓ 共 3056 个片段，总时长 12223.3s (203.7min)
[2026-07-09 19:28:51 INFO] [py] [curl_cffi_dl]    进度: 3056/3056 (成功 3056, 70s, 43.5 req/s)
[2026-07-09 19:28:51 INFO] [CF Bypass] ✓ M3U8 [1] 处理完成: ./downloads/0f6dce72-78a0-4397-aec2-44066a122831.mp4
```

### 📝 文件命名规则

工具会自动从 M3U8 URL 中提取有意义的标识作为输出文件名，避免哈希式的"乱码"名称：

| 命名方式             | 优先级    | 示例                                       |
| -------------------- | --------- | ------------------------------------------ |
| **从 URL 提取 UUID** | ⭐ 1      | `0f6dce72-78a0-4397-aec2-44066a122831.mp4` |
| URL 路径中的有意义段 | 2         | `xxx720p.mp4`（次选）                      |
| MD5 哈希（前 10 位） | 3（兜底） | `cf_9a9799a724.mp4`                        |

提取逻辑见 [src/main.rs:extract_video_id](src/main.rs)：

```rust
// English
// 1. UUID pattern: 8-4-4-4-12 hex (e.g., 0f6dce72-78a0-4397-aec2-44066a122831)
// 2. Last non-m3u8 path segment
// 3. MD5 fallback
```

### 性能数据（实测 2.5GB / 150 分钟视频）

| 阶段          | 耗时      | 说明                         |
| ------------- | --------- | ---------------------------- |
| M3U8 解析     | < 0.5s    | 单次 HTTP 请求               |
| TS 片段下载   | ~80s      | 24 并发 + Session 连接池复用 |
| cat 拼接      | ~2s       | 2.5GB 顺序写                 |
| ffmpeg 转封装 | ~15s      | `-c copy` 模式不转码         |
| **总耗时**    | **~100s** | 端到端                       |

### 添加新域名的旁路支持

编辑 [src/cloudflare_bypass.rs](src/cloudflare_bypass.rs)：

```rust
const CF_BYPASS_DOMAINS: &[&str] = &[
    "surrit.com",
    "fourhoi.com",
    "your-new-domain.com",  // 新增
];
```

### 临时清理

```bash
# 清理临时 work 目录
rm -rf downloads/cf_*.work

# 清理已下载的 MP4
rm -f downloads/cf_*.mp4
```

### 单独使用 Python 脚本

```bash
/Users/lst/miniconda3/bin/python3 scripts/curl_cffi_downloader.py \
  --url "https://surrit.com/xxx/720p/video.m3u8" \
  --referer "https://missav.ws/cn/xxx" \
  --output "./downloads/test.mp4" \
  --work-dir "/tmp/test_work" \
  --concurrent 24
```

## 🔧 开发工具

项目提供了完整的开发工具链：

```bash
# 设置开发环境
./scripts/dev-setup.sh

# 代码格式化
./scripts/format.sh

# 代码质量检查
./scripts/check.sh

# 运行测试
./scripts/test.sh

# 使用 Makefile (推荐)
make setup      # 环境设置
make check      # 质量检查
make test       # 运行测试
make doc        # 生成文档
make help       # 查看所有命令
```

## 📊 性能特性

### 自适应下载策略

- 根据网络带宽自动调整并发数
- 智能重试机制，避免无效重试
- 动态缓冲区管理

### 内存优化

- 流式处理，避免大文件占用内存
- 智能缓存管理
- 及时释放已处理的片段

### 硬件加速支持

- 自动检测可用的硬件加速器
- 支持 NVIDIA NVENC、Intel Quick Sync、AMD VCE
- 回退到软件编码确保兼容性

## 📁 项目结构

```
video_processor/
├── src/
│   ├── application/        # 应用层 - 业务逻辑协调
│   ├── domain/            # 领域层 - 核心业务逻辑
│   ├── infrastructure/    # 基础设施层 - 外部服务集成
│   ├── presentation/      # 表示层 - CLI 接口
│   ├── di/               # 依赖注入容器
│   ├── cloudflare_bypass.rs  # Cloudflare 旁路下载器（新）
│   └── main.rs           # 程序入口
├── scripts/
│   ├── dev-setup.sh
│   └── curl_cffi_downloader.py  # Python 下载器（新）
├── examples/             # 使用示例
├── docs/                # 文档
├── tests/               # 测试文件
├── config.yaml          # 配置文件
└── README.md
```

## 🐛 故障排除

### 常见问题

**1. FFmpeg 未找到**

```bash
# 检查 FFmpeg 是否安装
ffmpeg -version

# macOS 安装
brew install ffmpeg

# Ubuntu 安装
sudo apt install ffmpeg
```

**2. 下载速度慢**

- 调整 `max_concurrent_downloads` 参数
- 检查网络连接
- 尝试使用 `--copy` 模式避免重新编码

**3. Cloudflare 返回 403 Forbidden**

- 确认安装了 `curl-cffi`：`/Users/lst/miniconda3/bin/pip install curl-cffi`
- 确认 Python 路径：默认使用 `~/miniconda3/bin/python3`
- 临时验证：
  ```bash
  /Users/lst/miniconda3/bin/python3 -c "
  from curl_cffi import requests
  r = requests.get('https://surrit.com/...', impersonate='chrome', timeout=20)
  print('Status:', r.status_code)
  "
  ```
- 如返回 200 表示旁路正常；如仍 403 请检查代理/网络环境

**4. 内存使用过高**

- 减少 `buffer_size` 设置
- 降低 `max_concurrent_downloads`
- 启用 `validate_segments: false`

**5. 视频质量问题**

- 使用 `--copy` 模式保持原始质量
- 调整 `video_codec` 设置
- 检查源视频质量

### 日志调试

```bash
# 启用详细日志
export RUST_LOG=debug
cargo run --release

# 或在配置文件中设置
log_level: "debug"
```

## 🤝 贡献指南

1. Fork 项目
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

### 开发规范

- 遵循 Rust 官方代码风格
- 添加适当的测试用例
- 更新相关文档
- 运行 `make check` 确保代码质量

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## 🙏 致谢

- [FFmpeg](https://ffmpeg.org/) - 强大的多媒体处理框架
- [Tokio](https://tokio.rs/) - 异步运行时
- [Reqwest](https://github.com/seanmonstar/reqwest) - HTTP 客户端
- Rust 社区的所有贡献者

## 📞 支持

如果您遇到问题或有建议，请：

1. 查看 [故障排除](#-故障排除) 部分
2. 搜索现有的 [Issues](../../issues)
3. 创建新的 Issue 描述问题
4. 查看 [API 文档](docs/README.md) 获取详细信息

---

**注意**: 请确保您有权下载和处理目标视频内容，并遵守相关的版权法律法规。
