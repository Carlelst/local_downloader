# 使用官方的 Rust 镜像
FROM rust:latest

# 更新包列表并安装 FFmpeg
RUN apt-get update && apt-get install -y ffmpeg

# 安装 cargo-watch，用于实时监控代码更改
RUN cargo install cargo-watch

# 设置工作目录
WORKDIR /usr/src/app

# 预构建依赖时需要 Cargo.toml 和 Cargo.lock
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release || true

# 复制项目中所有代码
COPY . .

# 设置默认启动命令，使用 cargo-watch 监控 src 目录的代码变化并重新编译
CMD ["cargo", "watch", "-x", "build"]
