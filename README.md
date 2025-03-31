# 视频下载与解密处理器

本项目是一个异步 Rust 应用，用于从给定的 M3U8 URL 下载视频片段、解密 AES-128-CBC 加密的数据并通过 FFmpeg 合成输出文件。

## 功能概述

- 解析 M3U8 文件，获取 `.ts` 片段的 URL 列表
- 支持主播放列表和子播放列表递归解析
- 支持 AES-128-CBC 加密的片段解密，自动处理 IV
- 使用并发控制下载片段，提高下载效率
- 智能重试机制，使用指数退避算法处理临时性错误
- 使用缓冲区优化 I/O 操作，提升性能
- 使用 FFmpeg 合并解密后的片段为单个视频文件
- 详细的进度显示和日志输出
- 支持断点续传，可以恢复之前中断的下载
- 支持硬件加速编码，自动检测系统支持的加速器

## 安装与准备

### 系统要求

- Rust 1.50+
- FFmpeg (需要在系统中安装)

### 安装步骤

1. 克隆仓库：`git clone https://github.com/username/video-downloader.git`
2. 进入项目目录：`cd video-downloader`
3. 构建项目：`cargo build --release`

## 配置选项

项目通过 `config.yaml` 文件提供丰富的配置选项：

```yaml
m3u8_urls: # M3U8 播放列表 URL 数组
  - https://example.com/path/to/video.m3u8

# 下载配置
max_concurrent_downloads: 8 # 下载并行数
download_retries: 5 # 下载重试次数
segment_timeout: 40 # 每个片段下载的超时时间，单位：秒
retry_interval: 3 # 重试基础间隔时间，单位：秒

# 输出配置
output_directory: "./downloads" # 输出文件的目录
video_format: "mp4" # 输出视频格式，如 mp4, mkv, flv
video_codec: "copy" # 视频编码，通常使用 copy 以避免重新编码
audio_codec: "aac" # 音频编码，如 aac, copy, mp3

# 系统配置
log_level: "info" # 日志级别，可选："debug", "info", "warn", "error"
buffer_size: 4194304 # 缓冲区大小（字节），默认 4MB
validate_segments: false # 是否验证片段可访问性
use_hw_accel: true # 是否使用硬件加速
thread_count: 0 # FFmpeg线程数，0表示自动
video_bitrate: null # 视频比特率，如 "2M"，null表示使用原始比特率
audio_bitrate: null # 音频比特率，如 "128k"
chunk_size: 1048576 # 下载块大小（字节），默认1MB
```

## 使用方法

### 基本使用

1. 在 `config.yaml` 文件中配置 M3U8 URL 和其他选项
2. 运行应用程序：`cargo run --release`
3. 输出的视频文件将保存在指定的输出目录中（默认为 `./downloads`）

### 命令行参数

程序支持以下命令行参数，可以覆盖配置文件中的设置：

| 参数             | 描述                                   |
| ---------------- | -------------------------------------- |
| `--clear-cache`  | 清除所有下载缓存，重新开始下载         |
| `--force`        | 强制重新下载所有片段，忽略已缓存的片段 |
| `--bitrate=<值>` | 设置视频比特率，如 `--bitrate=2M`      |
| `--copy`         | 使用 copy 模式，不重新编码视频         |
| `--no-hw`        | 禁用硬件加速                           |

### 使用示例

#### 使用默认配置下载视频

```bash
cargo run --release
```

#### 清除缓存并重新下载

```bash
cargo run --release -- --clear-cache
```

#### 指定视频比特率

```bash
cargo run --release -- --bitrate=1M
```

#### 使用 copy 模式下载（不重新编码，保持原始质量）

```bash
cargo run --release -- --copy
```

#### 禁用硬件加速

```bash
cargo run --release -- --no-hw
```

#### 组合多个选项

```bash
cargo run --release -- --clear-cache --copy --no-hw
```

## 高级使用场景

### 1. 优化文件大小与质量

- **高质量模式**：使用 `--copy` 参数保留原始质量

  ```bash
  cargo run --release -- --copy
  ```

- **平衡模式**：使用中等比特率

  ```bash
  cargo run --release -- --bitrate=2M
  ```

- **压缩模式**：使用较低比特率减小文件大小
  ```bash
  cargo run --release -- --bitrate=1M
  ```

### 2. 性能优化

- **使用硬件加速**（默认开启）：
  系统会自动检测并使用可用的硬件加速器（VideoToolbox、NVENC、Intel QuickSync 等）

- **在低性能设备上禁用硬件加速**：
  ```bash
  cargo run --release -- --no-hw
  ```

### 3. 断点续传

程序默认支持断点续传，如果下载过程中断，再次运行时会自动从中断处继续下载。如果需要从头开始下载：

```bash
cargo run --release -- --clear-cache
```

### 4. 批量下载

在 `config.yaml` 文件中可以配置多个 M3U8 URL，程序会依次处理每个 URL：

```yaml
m3u8_urls:
  - https://example.com/video1.m3u8
  - https://example.com/video2.m3u8
  - https://example.com/video3.m3u8
```

## 实际应用案例

### 场景 1：下载课程视频并节省空间

假设您需要下载一个在线课程视频以便离线观看，但存储空间有限：

```bash
# 在config.yaml中设置课程URL，然后运行：
cargo run --release -- --bitrate=1M
```

这将以适中的质量下载视频，同时节省约 80%的存储空间。

### 场景 2：高质量备份流媒体内容

对于需要高质量备份的情况：

```bash
# 在config.yaml中设置目标URL，然后运行：
cargo run --release -- --copy --clear-cache
```

这将以原始质量下载并存档内容，确保最高的视频质量。

### 场景 3：在低性能设备上下载

在资源受限的设备上（如旧笔记本）：

```bash
cargo run --release -- --no-hw --bitrate=1M
```

这将禁用硬件加速并使用较低的比特率，减少处理负担。

## 故障排除

### 常见问题

1. **下载速度慢**

   - 尝试调整 `max_concurrent_downloads` 参数
   - 检查网络连接和服务器响应时间

2. **视频质量问题**

   - 使用 `--copy` 参数以避免重新编码
   - 调整比特率参数 `--bitrate=<值>`

3. **内存使用高**

   - 减小 `buffer_size` 和 `chunk_size` 参数
   - 减少 `max_concurrent_downloads` 值

4. **FFmpeg 错误**
   - 确保已安装最新版本的 FFmpeg
   - 使用 `--no-hw` 参数禁用硬件加速
   - 将日志级别设置为 `debug` 以获取更详细的错误信息

### 日志级别

通过修改 `config.yaml` 中的 `log_level` 参数可以控制输出的详细程度：

- `debug`：显示所有调试信息，包括下载细节和 FFmpeg 命令
- `info`：显示基本的进度和状态信息（默认）
- `warn`：只显示警告和错误
- `error`：只显示错误

## 目录结构

```plaintext
project_root/
├── Cargo.toml                    # 项目配置和依赖项
├── Dockerfile                    # 容器配置文件
├── docker-compose.yml            # 容器编排配置
├── config.yaml                   # 应用程序配置文件
├── src/
│   ├── main.rs                   # 主程序入口
│   ├── lib.rs                    # 公共模块定义和导入
│   ├── decryptor.rs              # 解密逻辑模块
│   ├── downloader.rs             # 下载和重试逻辑模块
│   ├── m3u8_parser.rs            # M3U8 解析逻辑模块
│   ├── ffmpeg.rs                 # FFmpeg 相关逻辑模块
│   ├── segment_processor.rs      # 段处理逻辑模块
└── tests/
    ├── integration_test.rs       # 集成测试
    └── unit/                     # 单元测试目录
```

## Docker 支持

项目提供了 Docker 支持，可以轻松地在容器中运行：

```bash
# 构建 Docker 镜像
docker build -t video-downloader .

# 运行容器
docker run -v $(pwd)/config.yaml:/usr/src/app/config.yaml -v $(pwd)/downloads:/usr/src/app/downloads video-downloader
```

## 支持的 URL 类型

- HLS 流媒体 URL (`.m3u8`)
- 加密和非加密的流
- 支持各种常见的视频托管平台

## 许可证

MIT

## Support

- https://surrit.com/xxx/video.m3u8
-
