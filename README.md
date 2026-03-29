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
max_concurrent_downloads: 16    # 最大并发下载数
download_retries: 3            # 下载重试次数
segment_timeout: 20            # 片段下载超时时间(秒)
retry_interval: 2              # 重试间隔(秒)
chunk_size: 2097152           # 下载块大小(字节，2MB)

# 输出配置
output_directory: "./downloads" # 输出目录
video_format: "mp4"            # 输出格式 (mp4, mkv, flv)
video_codec: "copy"            # 视频编码 (copy, h264, h265)
audio_codec: "aac"             # 音频编码 (aac, copy, mp3)

# 系统配置
log_level: "info"              # 日志级别 (debug, info, warn, error)
buffer_size: 8388608          # 缓冲区大小(字节，8MB)
validate_segments: false      # 是否验证片段
use_hw_accel: true           # 启用硬件加速
thread_count: 4              # FFmpeg 线程数
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
│   ├── application/     # 应用层 - 业务逻辑协调
│   ├── domain/         # 领域层 - 核心业务逻辑
│   ├── infrastructure/ # 基础设施层 - 外部服务集成
│   ├── presentation/   # 表示层 - CLI 接口
│   ├── di/            # 依赖注入容器
│   └── main.rs        # 程序入口
├── examples/          # 使用示例
├── scripts/           # 开发脚本
├── docs/             # 文档
├── tests/            # 测试文件
└── config.yaml       # 配置文件
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

**3. 内存使用过高**
- 减少 `buffer_size` 设置
- 降低 `max_concurrent_downloads`
- 启用 `validate_segments: false`

**4. 视频质量问题**
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