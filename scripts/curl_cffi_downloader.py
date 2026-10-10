#!/usr/bin/env python3
"""
使用 curl-cffi 模拟 Chrome 下载 M3U8 + 所有 TS 片段（用于绕过 Cloudflare）
被 Rust 主程序作为子进程调用。

参数：
  --url M3U8_URL       必填
  --referer URL        必填
  --output PATH        整片下载模式必填
  --work-dir PATH      临时目录（整片下载模式必填）
  --probe              预检模式：仅下载 URL 内容，原始字节写到 stdout 后退出
  --concurrent N       并发数（默认 12）
  --cookie STRING      可选 Cookie（部分 CF 站点仍需 cf_clearance）

严格保证：
  1. 先下载所有片段到磁盘
  2. 验证每个文件以 0x47 (TS sync) 开头
  3. 全部下载完成后才拼接为完整 TS
  4. ffmpeg 转封装为 MP4
"""
import argparse
import os
import sys
import shutil
import subprocess
import time
import json
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from urllib.parse import urljoin
from curl_cffi import requests

MIN_FILE_SIZE = 1000  # TS 段至少 1KB

HEADERS_TEMPLATE = {
    "sec-ch-ua-platform": "macOS",
    "sec-ch-ua": '"Google Chrome";v="149", "Chromium";v="149", "Not)A;Brand";v="24"',
    "sec-ch-ua-mobile": "?0",
}


def log(msg):
    print(f"[curl_cffi_dl] {msg}", file=sys.stderr, flush=True)


def concat_ts_files(work_dir: Path, output_path: Path) -> None:
    """用 Python 流式拼接所有 TS 片段（避免 shell=True 注入）"""
    with open(output_path, "wb") as out:
        for ts_file in sorted(work_dir.glob("*.ts")):
            with open(ts_file, "rb") as f:
                shutil.copyfileobj(f, out, length=1024 * 1024)  # 1MB buffer


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--referer", required=True)
    parser.add_argument("--output", default="")
    parser.add_argument("--work-dir", default="")
    parser.add_argument("--probe", action="store_true",
                        help="预检模式：仅下载 URL 内容，原始字节写到 stdout 后退出")
    parser.add_argument("--concurrent", type=int, default=12)
    parser.add_argument("--cookie", default="", help="可选 Cookie 字符串")

    args = parser.parse_args()

    headers = dict(HEADERS_TEMPLATE)
    headers["Referer"] = args.referer
    if args.cookie:
        headers["Cookie"] = args.cookie

    # ========== 预检模式：只取 playlist 资源 ==========
    if args.probe:
        r = requests.get(args.url, impersonate="chrome", headers=headers, timeout=30)
        if r.status_code != 200:
            log(f"❌ 预检下载失败: HTTP {r.status_code}")
            sys.exit(1)
        # 原始字节输出（可能是明文 m3u8 或 PNG 伪装资源，由调用方按内容探测解码）
        sys.stdout.buffer.write(r.content)
        sys.stdout.buffer.flush()
        return

    if not args.output or not args.work_dir:
        parser.error("整片下载模式需要 --output 与 --work-dir")

    work_dir = Path(args.work_dir)
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    work_dir.mkdir(parents=True, exist_ok=True)

    # ========== 阶段 1: 下载 M3U8 ==========
    log("阶段 1: 下载 M3U8")
    r = requests.get(args.url, impersonate="chrome", headers=headers, timeout=20)
    if r.status_code != 200:
        log(f"❌ M3U8 下载失败: {r.status_code}")
        sys.exit(1)
    m3u8_content = r.text
    log(f"✓ M3U8 下载成功 ({len(m3u8_content)} bytes)")

    # ========== 阶段 2: 解析片段 ==========
    log("阶段 2: 解析片段列表")
    segments = []
    current_duration = None
    for line in m3u8_content.split('\n'):
        line = line.strip()
        if line.startswith('#EXTINF:'):
            try:
                dur = float(line.split(':')[1].split(',')[0])
                current_duration = dur
            except Exception:
                current_duration = 4.0
        elif line and not line.startswith('#'):
            segments.append((line, current_duration or 4.0))
            current_duration = None

    total_duration = sum(d for _, d in segments)
    log(f"✓ 共 {len(segments)} 个片段，总时长 {total_duration:.1f}s ({total_duration/60:.1f}min)")

    # ========== 阶段 3: 并行下载所有 TS 片段 ==========
    log(f"阶段 3: 并行下载 {len(segments)} 个 TS 片段")

    # 关键优化：共享 Session + 连接池复用
    # 减少 TCP/TLS 握手次数（每个分片握手一次 → 全部连接复用）
    # curl_cffi 0.7+ 自动管理连接池，无需 max_clients 参数
    session = requests.Session(impersonate="chrome", headers=headers)
    log("   已创建共享 Session（连接池自动复用）")

    def download_one(idx, seg):
        filename, dur = seg
        seg_url = filename if filename.startswith('http') else urljoin(args.url, filename)
        save_path = work_dir / f"{idx:05d}.ts"
        if save_path.exists() and save_path.stat().st_size >= MIN_FILE_SIZE:
            with open(save_path, 'rb') as f:
                head = f.read(1)
            if head == b'\x47':
                return idx, True, "cached"
        for attempt in range(3):
            try:
                r = session.get(seg_url, timeout=30)
                if r.status_code == 200 and len(r.content) >= MIN_FILE_SIZE:
                    if r.content[0:1] != b'\x47':
                        return idx, False, f"not TS (header: {r.content[0:1].hex()})"
                    save_path.write_bytes(r.content)
                    return idx, True, f"{len(r.content)}B"
                time.sleep(1)
            except Exception:
                time.sleep(1)
        return idx, False, "failed"

    start = time.time()
    results = []
    with ThreadPoolExecutor(max_workers=args.concurrent) as ex:
        futs = {ex.submit(download_one, i, seg): i for i, seg in enumerate(segments)}
        for f in as_completed(futs):
            results.append(f.result())
            done = len(results)
            if done % 200 == 0 or done == len(segments):
                elapsed = time.time() - start
                rate = done / elapsed if elapsed > 0 else 0
                success_count = sum(1 for _, s, _ in results if s)
                log(f"   进度: {done}/{len(segments)} (成功 {success_count}, {elapsed:.0f}s, {rate:.1f} req/s)")

    success = sum(1 for _, s, _ in results if s)
    failed = [r for r in results if not r[1]]
    log(f"✓ 下载完成: {success}/{len(segments)} 成功")
    if failed:
        log(f"❌ {len(failed)} 个失败")
        for idx, _, info in failed[:5]:
            log(f"   片段 {idx}: {info}")
        sys.exit(1)

    # ========== 阶段 4: 验证完整性 ==========
    log("阶段 4: 验证所有片段完整性")
    all_ok = True
    total_size = 0
    for i in range(len(segments)):
        path = work_dir / f"{i:05d}.ts"
        if not path.exists():
            log(f"❌ 缺失: {path}")
            all_ok = False
            continue
        size = path.stat().st_size
        total_size += size
        if size < MIN_FILE_SIZE:
            log(f"❌ 过小 ({size}B): {path}")
            all_ok = False
        with open(path, 'rb') as f:
            head = f.read(1)
        if head != b'\x47':
            log(f"❌ 非 TS 格式: {path} (header: {head.hex()})")
            all_ok = False

    if not all_ok:
        log("❌ 完整性校验失败")
        sys.exit(1)
    log(f"✓ 全部 {len(segments)} 个 TS 片段验证通过")
    log(f"   总大小: {total_size/1024/1024:.1f} MB")

    # ========== 阶段 5: 拼接为单个 TS ==========
    log("阶段 5: 拼接所有 TS 片段（Python 流式，无需 shell）")
    ts_output = output_path.with_suffix(".merged.ts")
    concat_ts_files(work_dir, ts_output)
    ts_size = ts_output.stat().st_size / 1024 / 1024
    log(f"✓ 拼接完成: {ts_output} ({ts_size:.1f} MB)")

    # ========== 阶段 6: TS 转 MP4 ==========
    log("阶段 6: TS 转 MP4（仅转封装，不转码）")
    cmd = [
        'ffmpeg', '-y', '-i', str(ts_output),
        '-c', 'copy',
        '-movflags', '+faststart',
        str(output_path)
    ]
    # 关键：stdin=DEVNULL 避免 ffmpeg 在 tcsetattr/ioctl 上永久阻塞
    # （ffmpeg 启动时尝试设置 TTY，pipe 不是 TTY 会卡死）
    # stderr 直接写日志文件，不通过 pipe（capture_output 会卡）
    ffmpeg_log = ts_output.with_suffix(".ffmpeg.log")
    with open(ffmpeg_log, 'w') as fflog:
        p = subprocess.run(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=fflog,
            stderr=subprocess.STDOUT,
            timeout=600,
            shell=False,  # 显式禁用 shell，避免注入
        )
    if p.returncode != 0:
        try:
            with open(ffmpeg_log, 'r') as fflog:
                err = fflog.read()[-1500:]
        except Exception:
            err = "(无法读取 ffmpeg 日志)"
        log(f"❌ ffmpeg 失败:\n{err}")
        sys.exit(1)

    size_mb = output_path.stat().st_size / 1024 / 1024
    log(f"✓ 完成: {output_path} ({size_mb:.2f} MB, 总耗时 {time.time()-start:.1f}s)")

    # 清理临时文件（用 pathlib，零 shell 注入风险）
    try:
        ts_output.unlink(missing_ok=True)
        ffmpeg_log.unlink(missing_ok=True)
        shutil.rmtree(work_dir, ignore_errors=True)
        log("   已清理临时文件")
    except Exception as e:
        warn(f"清理临时文件失败（可忽略）: {e}")

    # 验证
    result = subprocess.run(
        ['ffprobe', '-v', 'error', '-show_entries',
         'format=duration:stream=width,height,codec_name', '-of', 'json',
         str(output_path)],
        capture_output=True, text=True,
        shell=False,
    )
    if result.returncode == 0:
        info = json.loads(result.stdout)
        if 'streams' in info and info['streams']:
            s = info['streams'][0]
            log(f"   视频: {s.get('codec_name')} {s.get('width')}x{s.get('height')}")
        if 'format' in info:
            log(f"   时长: {float(info['format']['duration']):.1f}s")


def warn(msg):
    print(f"[curl_cffi_dl] WARN: {msg}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
