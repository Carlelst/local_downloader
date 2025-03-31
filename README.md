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

## 配置选项

项目通过 `config.yaml` 文件提供丰富的配置选项：

```yaml
m3u8_urls: # M3U8 播放列表 URL 数组
  -

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
```

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

## 最近优化

1. **增强配置系统**：添加了更多配置选项，包括视频格式、编码器选择等
2. **智能重试机制**：实现指数退避算法，更有效地处理临时网络故障
3. **缓冲区优化**：使用 BufWriter 减少系统调用，提高 I/O 效率
4. **改进 M3U8 解析**：支持主播放列表和递归解析，更好地处理各种格式
5. **更好的错误处理**：提供更详细的错误信息和恢复策略
6. **进度报告**：添加详细的下载进度显示
7. **性能优化**：减少不必要的内存分配和系统调用

## 使用方法

1. 在 `config.yaml` 文件中配置 M3U8 URL 和其他选项
2. 运行应用程序：`cargo run --release`
3. 输出的视频文件将保存在指定的输出目录中

## 支持的 URL 类型

- HLS 流媒体 URL (`.m3u8`)
- 加密和非加密的流
- 支持各种常见的视频托管平台

## 依赖项

- FFmpeg (需要在系统中安装)
- Rust 1.50+

## 许可证

MIT

## Support

- https://surrit.com/xxx/video.m3u8
-
