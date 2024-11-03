# 使用官方的 Rust 镜像
FROM rust:latest

# 更新包列表并安装 FFmpeg
RUN apt-get update && apt-get install -y ffmpeg

# 安装 cargo-watch，用于实时监控代码更改
RUN cargo install cargo-watch

# 设置工作目录
WORKDIR /usr/src/app

# Cargo.toml需要cargo init ,Cargo.lock需要cargo build
# 将 Cargo.toml 和 Cargo.lock 复制到容器中，以便进行依赖预构建
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release || true

# 复制所有代码
COPY . .

# 设置默认启动命令，使用 cargo-watch 监控 src 目录的代码变化并重新运行
# CMD ["cargo", "watch", "-x", "run"]
CMD ["cargo", "watch", "-x", "build"]
# CMD ["cargo", "run"]