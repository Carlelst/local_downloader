# 视频下载与解密处理器

本项目是一个异步 Rust 应用，用于从给定的 M3U8 URL 下载视频片段、解密 AES-128-CBC 加密的数据并通过 FFmpeg 合成输出文件。

## 功能概述

- 解析 M3U8 文件，获取 `.ts` 片段的 URL 列表
- 支持 AES-128-CBC 加密的片段解密
- 使用并发控制下载片段，提高下载效率
- 使用 FFmpeg 合并解密后的片段为单个视频文件

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
│   ├── utils.rs                  # 实用工具和辅助函数模块
└── tests/
    ├── integration_test.rs       # 集成测试
    ├── downloader_test.rs        # 下载模块的单元测试
    ├── m3u8_parser_test.rs       # M3U8 解析模块的单元测试
    └── ffmpeg_test.rs            # FFmpeg 相关逻辑的单元测试

```

## Support
- https://surrit.com/xxx/video.m3u8
- 