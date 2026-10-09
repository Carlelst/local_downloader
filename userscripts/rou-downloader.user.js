// ==UserScript==
// @name         rou.video 悬浮球下载器
// @namespace    https://rou.video/
// @version      1.8.0
// @description  在 rou.video 播放页右下角显示悬浮球：自动捕获视频伪装 playlist URL（含实时 auth），点击一键发送到本地 video_processor --server 下载
// @match        https://rou.video/*
// @match        https://www.rou.video/*
// @match        https://missav.ws/*
// @match        https://www.missav.ws/*
// @match        https://missav.ai/*
// @match        https://www.missav.ai/*
// @run-at       document-start
// @grant        GM_xmlhttpRequest
// @grant        GM_setValue
// @grant        GM_getValue
// @grant        unsafeWindow
// @connect      127.0.0.1
// @connect      localhost
// @connect      *
// @updateURL    https://raw.githubusercontent.com/Carlelst/local_downloader/master/userscripts/rou-downloader.user.js
// @downloadURL  https://raw.githubusercontent.com/Carlelst/local_downloader/master/userscripts/rou-downloader.user.js
// ==/UserScript==

(function () {
    'use strict';

    const SERVER = 'http://127.0.0.1:7777';
    // 伪装 playlist 的请求模式：/hls/<id>/<variant>/index.png|jpg（带认证 query）
    const PLAYLIST_RE = /\/hls\/[^/?#\s]+\/[^/?#\s]+\/index\.(png|jpg)\?/i;
    // 标准 m3u8（missav 等，hls.js 的 XHR/fetch 请求）
    const M3U8_RE = /\.m3u8(\?|#|$)/i;
    // missav 页面源码里 eval 混淆的 m3u8 字符串数组（"m3u8|" 拼接），网络 hook 抓不到时兜底
    const SOURCE_RE = /["'](https?:\/\/[^"']+\.m3u8[^"']*)["']/i;

    let capturedUrl = null;
    let ballEl = null; // 球在 shadow DOM 里，document.querySelector 查不到，保存直接引用

    // 播放器可能在 iframe 里（Network 面板有请求但顶层 performance 记录里没有）：
    // Tampermonkey 默认向所有 frame 注入本脚本 → 每个 frame 各自捕获，iframe 命中后逐层上报顶层
    const IS_TOP = (() => { try { return window.top === window; } catch (e) { return false; } })();

    function capture(url, source) {
        if (!url || typeof url !== 'string') return;
        const clean = url.split('#')[0];
        if (M3U8_RE.test(clean) || PLAYLIST_RE.test(clean)) {
            if (capturedUrl !== clean) {
                capturedUrl = clean;
                // 捕获成功提示：球变绿 + 控制台日志，用户无需猜测
                if (ballEl) ballEl.style.background = 'linear-gradient(135deg, #66bb6a, #2e7d32)';
                console.log('[rou-dl] ✓ 已捕获视频地址 (' + source + '):', clean);
                reportToTop(clean);
            }
        }
    }

    // 非顶层 frame：把捕获的地址逐层上报给顶层（嵌套 iframe 由中间层监听后继续转发）
    function reportToTop(url) {
        if (IS_TOP) return;
        try { window.parent.postMessage({ __rdCapture: url }, '*'); } catch (e) { /* 忽略 */ }
    }

    // 从任意文本（JSON 响应/页面源码）里提取视频地址；返回是否命中
    function captureFromText(text, source) {
        if (!text || typeof text !== 'string' || text.length > 3_000_000) return false;
        const m =
            text.match(new RegExp('"[^"]*' + PLAYLIST_RE.source.slice(1, -2) + '[^"]*"')) || // JSON 里的伪装地址
            text.match(SOURCE_RE) ||
            text.match(/https?:\/\/[^\s"']+\.m3u8[^\s"']*/i);
        if (m) { capture(m[0].replace(/^"|"$/g, ''), source); return true; }
        return false;
    }

    // 页面加载完成后扫一遍 script 源码（m3u8/伪装地址常藏在 eval 拼接或内联 JSON 里）
    function scanPageSource() {
        try {
            captureFromText(document.documentElement.innerHTML, '页面源码');
        } catch (e) { /* 忽略 */ }
    }

    // 扫描浏览器网络记录（performance resource entries）：
    // 记录页面发出的所有请求（含 Web Worker 发出的、含完整 query），不依赖 hook 时序，
    // 是 hook 失效时（bundle 提前缓存原生 fetch / MSE 内部请求）最可靠的兜底
    function scanPerformance() {
        try {
            const entries = performance.getEntriesByType('resource');
            for (let i = entries.length - 1; i >= 0; i--) {
                const u = (entries[i].name || '').split('#')[0];
                if (PLAYLIST_RE.test(u) || M3U8_RE.test(u)) {
                    capture(u, '网络记录');
                    return true;
                }
            }
        } catch (e) { /* 忽略 */ }
        return false;
    }

    // ---------- 1. hook fetch / XHR，捕获 playlist URL ----------
    // 关键：脚本有 @grant 时运行在 Tampermonkey 沙箱，window 是沙箱代理，
    // 页面播放器用的是 unsafeWindow 里的原生 fetch/XHR，必须 hook unsafeWindow 才拦得到

    const pageWindow = typeof unsafeWindow !== 'undefined' ? unsafeWindow : window;

    const origFetch = pageWindow.fetch;
    pageWindow.fetch = function (input, init) {
        try {
            const url = typeof input === 'string' ? input : (input && input.url) || '';
            capture(url, 'fetch');
        } catch (e) { /* 忽略 hook 异常，不影响页面 */ }
        const p = origFetch.apply(this, arguments);
        // 响应体扫描兜底：playlist 地址常藏在 API 的 JSON 响应里（如 /api/.../source）
        p.then((resp) => {
            try {
                if (resp && resp.ok && (resp.type === 'basic' || resp.type === 'cors')) {
                    resp.clone().text().then((t) => captureFromText(t, 'fetch响应')).catch(() => {});
                }
            } catch (e) { /* 忽略 */ }
        }).catch(() => {});
        return p;
    };

    const origOpen = pageWindow.XMLHttpRequest.prototype.open;
    const origSend = pageWindow.XMLHttpRequest.prototype.send;
    pageWindow.XMLHttpRequest.prototype.open = function (method, url) {
        try {
            this.__rdUrl = typeof url === 'string' ? url : '';
            capture(this.__rdUrl, 'xhr');
        } catch (e) { /* 忽略 */ }
        return origOpen.apply(this, arguments);
    };
    pageWindow.XMLHttpRequest.prototype.send = function () {
        try {
            this.addEventListener('load', () => {
                try {
                    const ct = this.getResponseHeader('content-type') || '';
                    if (/json|text|javascript/i.test(ct)) {
                        captureFromText(this.responseText, 'xhr响应');
                    }
                } catch (e) { /* 忽略 */ }
            });
        } catch (e) { /* 忽略 */ }
        return origSend.apply(this, arguments);
    };

    // ---------- 1.5 网络记录轮询：每 2s 扫一次，hook 失效时的主力捕获通道 ----------
    const perfPoll = setInterval(() => {
        if (capturedUrl || scanPerformance()) clearInterval(perfPoll);
    }, 2000);
    window.addEventListener('load', () => {
        scanPerformance();
        // load 后 3s 自动跑一次 API 重放（页面 API 响应若含播放源，无需用户点球即可捕获）
        setTimeout(() => { if (!capturedUrl) captureFromApiReplay(); }, 3000);
    });

    // ---------- 1.6 API 重放兜底：hook 时机不可靠时的终极通道 ----------
    // 页面加载时顶层 JS 必定调用过本站 API 获取播放源（auth/exp 就来自它的响应），
    // 该请求留在顶层 performance 记录里；用 GM_xmlhttpRequest 重放（不走页面 hook，不受时机影响），
    // 扫响应体提取视频地址
    async function captureFromApiReplay() {
        if (capturedUrl) return true;
        try {
            const entries = performance.getEntriesByType('resource')
                .filter((e) => e.initiatorType === 'fetch' || e.initiatorType === 'xmlhttprequest');
            const seen = new Set();
            for (const e of entries) {
                if (capturedUrl) return true;
                const u = (e.name || '').split('#')[0];
                if (!u || seen.has(u)) continue;
                seen.add(u);
                // 只重放本站 API（跨域资源重放无意义且量大）
                if (!u.startsWith(location.origin + '/')) continue;
                if (!/\/api|\/video|\/source|\/play|\/v\//i.test(u)) continue;
                const text = await new Promise((res) => {
                    try {
                        GM_xmlhttpRequest({
                            method: 'GET', url: u, timeout: 8000,
                            onload: (r) => {
                                // 302 场景：最终 URL 就是带 auth 的播放资源（kamq40.xyz/hls/...）
                                if (r && r.finalUrl) capture(r.finalUrl, 'API重放-重定向');
                                res((r && r.responseText) || '');
                            },
                            onerror: (r) => {
                                // 被 @connect 拒绝等异常时也尝试从 finalUrl 捞重定向目标
                                if (r && r.finalUrl) capture(r.finalUrl, 'API重放-异常重定向');
                                res('');
                            },
                            ontimeout: () => res(''),
                        });
                    } catch (err) { res(''); }
                });
                if (text && captureFromText(text, 'API重放')) {
                    console.log('[rou-dl] API 重放命中:', u);
                    return true;
                }
            }
        } catch (e) { /* 忽略 */ }
        return false;
    }

    // 点球诊断：列出页面 iframe 的 src（播放器若在第三方 iframe，其域名需要补进 @match）
    function diagnoseIframes() {
        try {
            const frames = [...document.querySelectorAll('iframe')]
                .map((f) => f.src || '').filter((s) => /^https?:/i.test(s));
            if (frames.length) console.log('[rou-dl] 页面 iframe 列表:', frames);
            else console.log('[rou-dl] 页面无 iframe');
        } catch (e) { /* 忽略 */ }
    }

    // ---------- 2. 悬浮球 UI ----------

    function mount() {
        const ball = document.createElement('div');
        ball.id = 'rou-dl-ball';
        ball.textContent = '↓';
        Object.assign(ball.style, {
            position: 'fixed',
            right: '24px',
            bottom: '24px',
            zIndex: '2147483647',
            width: '52px',
            height: '52px',
            borderRadius: '50%',
            background: 'linear-gradient(135deg, #4f8cff, #2b5fd9)',
            color: '#fff',
            fontSize: '26px',
            lineHeight: '52px',
            textAlign: 'center',
            cursor: 'pointer',
            userSelect: 'none',
            boxShadow: '0 4px 16px rgba(0,0,0,.4)',
            opacity: '0.85',
            transition: 'opacity .2s, transform .2s',
        });
        ball.addEventListener('mouseenter', () => {
            ball.style.opacity = '1';
            // 交互增强：悬停时显示当前捕获状态
            if (capturedUrl) showTip('已捕获: ' + capturedUrl.slice(0, 120), '#81c784');
        });
        ball.addEventListener('mouseleave', () => (ball.style.opacity = '0.85'));

        // 消息提示条
        const tip = document.createElement('div');
        Object.assign(tip.style, {
            position: 'fixed',
            right: '24px',
            bottom: '88px',
            zIndex: '2147483647',
            maxWidth: '320px',
            padding: '8px 12px',
            borderRadius: '8px',
            background: 'rgba(0,0,0,.85)',
            color: '#fff',
            fontSize: '13px',
            lineHeight: '1.5',
            display: 'none',
            boxShadow: '0 4px 16px rgba(0,0,0,.4)',
            wordBreak: 'break-all',
        });

        // 拖动（区分点击：移动超过 6px 视为拖动）
        let dragging = false, moved = false, startX = 0, startY = 0;
        ball.addEventListener('mousedown', (e) => {
            dragging = true; moved = false;
            startX = e.clientX; startY = e.clientY;
            e.preventDefault();
        });
        document.addEventListener('mousemove', (e) => {
            if (!dragging) return;
            const dx = e.clientX - startX, dy = e.clientY - startY;
            if (Math.abs(dx) + Math.abs(dy) > 6) moved = true;
            if (moved) {
                ball.style.right = Math.max(0, parseInt(ball.style.right) - dx) + 'px';
                ball.style.bottom = Math.max(0, parseInt(ball.style.bottom) - dy) + 'px';
                startX = e.clientX; startY = e.clientY;
            }
        });
        document.addEventListener('mouseup', () => (dragging = false));

        ball.addEventListener('click', () => {
            if (moved) return; // 拖动结束不触发下载
            submit();
        });

        function showTip(text, color) {
            tip.textContent = text;
            tip.style.borderLeft = `4px solid ${color || '#4f8cff'}`;
            tip.style.display = 'block';
            clearTimeout(showTip._t);
            showTip._t = setTimeout(() => (tip.style.display = 'none'), 5000);
        }

        function setBusy(busy) {
            ball.textContent = busy ? '…' : '↓';
            ball.style.pointerEvents = busy ? 'none' : 'auto';
            ball.style.transform = busy ? 'scale(.9)' : 'none';
        }

        // 防抖：提交进行中直接忽略后续点击（配合 busy 的 pointerEvents 双保险）
        let submitting = false;

        async function submit() {
            if (submitting) return;
            if (!capturedUrl) {
                // 点球时终极兜底：源码 → 网络记录 → API 重放，全走一遍
                scanPageSource();
                scanPerformance();
                diagnoseIframes();
                await captureFromApiReplay();
            }
            if (!capturedUrl) {
                showTip('尚未捕获到视频地址：已尝试源码/网络记录/API 重放。请把控制台 [rou-dl] 的 iframe 列表发开发者', '#ffb74d');
                return;
            }
            submitting = true;
            setBusy(true);
            showTip('正在发送到本地下载器…', '#4f8cff');
            GM_xmlhttpRequest({
                method: 'POST',
                url: SERVER + '/download',
                headers: { 'Content-Type': 'application/json' },
                data: JSON.stringify({ url: capturedUrl }),
                timeout: 30000,
                onload: (resp) => {
                    submitting = false;
                    setBusy(false);
                    let data = {};
                    try { data = JSON.parse(resp.responseText); } catch (e) { /* 忽略 */ }
                    if (resp.status === 200 && data.ok) {
                        showTip(`✓ 已加入下载队列（${data.segments} 个分片），进度见服务端终端`, '#81c784');
                    } else {
                        showTip(`✗ 失败: ${data.error || 'HTTP ' + resp.status}`, '#e57373');
                    }
                },
                onerror: () => {
                    submitting = false;
                    setBusy(false);
                    showTip('✗ 连不上本地服务：请先运行 ./target/release/video_processor --server', '#e57373');
                },
                ontimeout: () => {
                    submitting = false;
                    setBusy(false);
                    showTip('✗ 本地服务响应超时', '#e57373');
                },
            });
        }

        function mountBall() {
            const host = document.getElementById('rd-x9k2-host');
            if (host) return true;
            if (!document.documentElement) return false;
            // 用 Shadow DOM + 中性命名：广告拦截器（AdGuard/uBlock）的元素隐藏规则
            // 会把 id 含 ball/float/download 的悬浮元素当广告隐藏，CSS 规则进不了 shadow root
            const container = document.createElement('div');
            container.id = 'rd-x9k2-host';
            const shadow = container.attachShadow({ mode: 'open' });
            const style = document.createElement('style');
            style.textContent = `
                :host { all: initial; }
                .ball { position: fixed; right: 24px; bottom: 24px; z-index: 2147483647;
                        width: 52px; height: 52px; border-radius: 50%;
                        background: linear-gradient(135deg, #4f8cff, #2b5fd9);
                        color: #fff; font: 26px/52px sans-serif; text-align: center;
                        cursor: pointer; user-select: none;
                        box-shadow: 0 4px 16px rgba(0,0,0,.4); opacity: .85;
                        transition: opacity .2s, transform .2s;
                        display: block !important; visibility: visible !important; }
                .tip { position: fixed; right: 24px; bottom: 88px; z-index: 2147483647;
                       max-width: 320px; padding: 8px 12px; border-radius: 8px;
                       background: rgba(0,0,0,.85); color: #fff; font: 13px/1.5 sans-serif;
                       display: none; box-shadow: 0 4px 16px rgba(0,0,0,.4); word-break: break-all; }
            `;
            shadow.append(style, ball, tip);
            document.documentElement.append(container);
            ballEl = ball;
            // 运行标记：F12 控制台能看到这行说明脚本已生效
            console.log('[rou-dl] 悬浮球已挂载 @', location.href);
            // 自检：1.5s 后打印实际渲染状态，便于排查"看不到球"（host 可能已被页面重建，判空防报错）
            setTimeout(() => {
                const el = shadow.querySelector('.ball');
                if (!el) return;
                const r = el.getBoundingClientRect();
                const cs = getComputedStyle(el);
                console.log('[rou-dl] 自检: rect=', JSON.stringify({ x: r.x, y: r.y, w: r.width, h: r.height }),
                    'display=', cs.display, 'visibility=', cs.visibility, 'opacity=', cs.opacity);
            }, 1500);
            return true;
        }
        // document-start 时 html 尚未就绪：先试一次，DOMContentLoaded 再试
        const domReady = () => {
            scanPageSource();
            mountBall() || setTimeout(mountBall, 2000);
        };
        if (!mountBall()) {
            document.addEventListener('DOMContentLoaded', domReady);
            window.addEventListener('load', domReady);
        } else {
            window.addEventListener('load', scanPageSource);
        }

        // 防移除观察器：SPA 重渲染/清理 DOM 后自动补挂（悬浮球"消失"最常见的原因）
        const observer = new MutationObserver(() => {
            if (!document.getElementById('rd-x9k2-host')) {
                console.log('[rou-dl] 悬浮球被页面移除，重新挂载');
                mountBall();
            }
        });
        const startObserver = () => observer.observe(document.documentElement, { childList: true, subtree: false });
        if (document.documentElement) startObserver();
        else document.addEventListener('DOMContentLoaded', startObserver, { once: true });
    }

    try {
        // 接收 iframe 捕获上报；中间层收到后再向上一层转发，最终到达顶层球
        window.addEventListener('message', (e) => {
            const d = e.data;
            if (d && typeof d === 'object' && d.__rdCapture) {
                capture(d.__rdCapture, 'iframe(' + e.origin + ')');
            }
        });

        if (IS_TOP) {
            mount(); // 悬浮球只挂在顶层；iframe 实例只负责捕获+上报
        }
    } catch (e) {
        console.error('[rou-dl] 悬浮球初始化失败:', e);
    }
})();
