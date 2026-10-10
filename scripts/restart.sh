#!/bin/bash
# 重新编译并重启下载服务（LaunchAgent 托管）——改完 Rust 代码后跑这个
set -e
cd "$(dirname "$0")/.."
cargo build --release
launchctl kickstart -k "gui/$(id -u)/com.localdownloader.server"
sleep 1
if curl -sf --max-time 3 http://127.0.0.1:7777/health > /dev/null; then
    echo "✓ server 已用新二进制重启（http://127.0.0.1:7777）"
else
    echo "✗ server 未响应，查看 logs/server.err.log"
    exit 1
fi
