// ==UserScript==
// @name         rou.video 悬浮球下载器
// @namespace    https://rou.video/
// @version      2.0.0
// @description  在 rou.video / missav 播放页右下角显示悬浮球：自动捕获视频地址并暂存为任务（跨页面持久化），点击球打开管理面板，勾选批量发送到本地 video_processor --server 下载
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
    const TASKS_KEY = 'rd_tasks_v2';
    // 伪装 playlist 的请求模式：/hls/<id>/<variant>/index.png|jpg（带认证 query）
    const PLAYLIST_RE = /\/hls\/[^/?#\s]+\/[^/?#\s]+\/index\.(png|jpg)\?/i;
    // 标准 m3u8（missav 等，hls.js 的 XHR/fetch 请求）
    const M3U8_RE = /\.m3u8(\?|#|$)/i;
    // missav 页面源码里 eval 混淆的 m3u8 字符串数组（"m3u8|" 拼接），网络 hook 抓不到时兜底
    const SOURCE_RE = /["'](https?:\/\/[^"']+\.m3u8[^"']*)["']/i;

    // 播放器可能在 iframe 里：Tampermonkey 默认向所有 frame 注入本脚本，
    // 每个 frame 各自捕获（直接写 GM 存储，天然跨 frame 共享），iframe 命中后同时 postMessage 通知顶层刷新角标
    const IS_TOP = (() => { try { return window.top === window; } catch (e) { return false; } })();

    // ================= 任务存储（GM 持久化，跨页面/跨 frame 共享） =================

    function loadTasks() {
        try {
            const raw = GM_getValue(TASKS_KEY, '[]');
            const arr = JSON.parse(raw);
            return Array.isArray(arr) ? arr : [];
        } catch (e) { return []; }
    }
    function saveTasks(tasks) {
        try { GM_setValue(TASKS_KEY, JSON.stringify(tasks)); } catch (e) { /* 忽略 */ }
    }
    function taskId(url) {
        // 同一 URL 同一任务：用 URL 稳定哈希做 id（不同页面重复捕获自动合并）
        let h = 0;
        for (let i = 0; i < url.length; i++) { h = (h * 31 + url.charCodeAt(i)) | 0; }
        return 't' + (h >>> 0).toString(36);
    }
    /** 捕获入队：同 URL 去重；返回是否新增/刷新 */
    function addTask(url, title) {
        const tasks = loadTasks();
        const id = taskId(url);
        const existing = tasks.find((t) => t.id === id);
        // pending/sending 中不重复入队；sent/failed 再次捕获 = 用户想重下（auth 可能已刷新）→ 重置为 pending
        if (existing && (existing.status === 'pending' || existing.status === 'sending')) return false;
        tasks.unshift({
            id,
            url,
            title: (title || '').slice(0, 120) || url.slice(0, 80),
            host: (() => { try { return new URL(url).host; } catch (e) { return ''; } })(),
            t: Date.now(),
            status: 'pending', // pending | sending | sent | failed
            err: '',
        });
        saveTasks(tasks.slice(0, 200)); // 上限 200 条防膨胀
        return true;
    }
    function updateTask(id, patch) {
        const tasks = loadTasks();
        const t = tasks.find((x) => x.id === id);
        if (t) { Object.assign(t, patch); saveTasks(tasks); }
    }
    function removeTasks(ids) {
        const set = new Set(ids);
        saveTasks(loadTasks().filter((t) => !set.has(t.id)));
    }
    function pendingCount() {
        return loadTasks().filter((t) => t.status === 'pending').length;
    }

    // ================= 捕获（五层通道，与 v1.8 相同） =================

    let ballEl = null;      // 球在 shadow DOM 里，保存直接引用
    let badgeEl = null;     // 角标
    let onTasksChanged = null; // 任务变动回调（顶层用于刷新角标/面板）

    function notifyTasksChanged() {
        if (onTasksChanged) { try { onTasksChanged(); } catch (e) { /* 忽略 */ } }
    }

    function capture(url, source) {
        if (!url || typeof url !== 'string') return;
        const clean = url.split('#')[0];
        if (M3U8_RE.test(clean) || PLAYLIST_RE.test(clean)) {
            const added = addTask(clean, document.title);
            if (added) {
                console.log('[rou-dl] ✓ 已捕获并暂存 (' + source + '):', clean);
                notifyTasksChanged();
                reportToTop(clean);
            }
        }
    }

    // 非顶层 frame：捕获后通知顶层立即刷新角标
    function reportToTop(url) {
        if (IS_TOP) return;
        try { window.parent.postMessage({ __rdCapture: url }, '*'); } catch (e) { /* 忽略 */ }
    }

    // 从任意文本（JSON 响应/页面源码）里提取视频地址
    function captureFromText(text, source) {
        if (!text || typeof text !== 'string' || text.length > 3_000_000) return;
        const m =
            text.match(new RegExp('"[^"]*' + PLAYLIST_RE.source.slice(1, -2) + '[^"]*"')) || // JSON 里的伪装地址
            text.match(SOURCE_RE) ||
            text.match(/https?:\/\/[^\s"']+\.m3u8[^\s"']*/i);
        if (m) capture(m[0].replace(/^"|"$/g, ''), source);
    }

    // 页面加载完成后扫一遍 script 源码
    function scanPageSource() {
        try {
            captureFromText(document.documentElement.innerHTML, '页面源码');
        } catch (e) { /* 忽略 */ }
    }

    // 扫描浏览器网络记录（含 Web Worker 发出的请求），hook 失效时的主力兜底
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

    // ---------- 1. hook fetch / XHR ----------
    // 关键：脚本有 @grant 时运行在沙箱，必须 hook unsafeWindow 才拦得到页面请求
    const pageWindow = typeof unsafeWindow !== 'undefined' ? unsafeWindow : window;

    const origFetch = pageWindow.fetch;
    pageWindow.fetch = function (input, init) {
        try {
            const url = typeof input === 'string' ? input : (input && input.url) || '';
            capture(url, 'fetch');
        } catch (e) { /* 忽略 hook 异常，不影响页面 */ }
        const p = origFetch.apply(this, arguments);
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
            capture(typeof url === 'string' ? url : '', 'xhr');
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

    // ---------- 1.5 网络记录轮询 ----------
    const perfPoll = setInterval(() => {
        if (scanPerformance()) clearInterval(perfPoll);
    }, 2000);

    // ---------- 1.6 API 重放兜底 ----------
    // 页面加载时顶层 JS 必定调用过本站 API 获取播放源；GM_xmlhttpRequest 重放
    // （不走页面 hook，不受注入时机影响），302 重定向目标就是带 auth 的播放 URL
    async function captureFromApiReplay() {
        try {
            const entries = performance.getEntriesByType('resource')
                .filter((e) => e.initiatorType === 'fetch' || e.initiatorType === 'xmlhttprequest');
            const seen = new Set();
            for (const e of entries) {
                const u = (e.name || '').split('#')[0];
                if (!u || seen.has(u)) continue;
                seen.add(u);
                if (!u.startsWith(location.origin + '/')) continue;
                if (!/\/api|\/video|\/source|\/play|\/v\//i.test(u)) continue;
                const before = loadTasks().length;
                await new Promise((res) => {
                    try {
                        GM_xmlhttpRequest({
                            method: 'GET', url: u, timeout: 8000,
                            onload: (r) => {
                                if (r && r.finalUrl) capture(r.finalUrl, 'API重放-重定向');
                                res((r && r.responseText) || '');
                            },
                            onerror: (r) => {
                                if (r && r.finalUrl) capture(r.finalUrl, 'API重放-异常重定向');
                                res('');
                            },
                            ontimeout: () => res(''),
                        });
                    } catch (err) { res(''); }
                });
                if (loadTasks().length > before) { console.log('[rou-dl] API 重放命中:', u); return true; }
            }
        } catch (e) { /* 忽略 */ }
        return false;
    }

    // ---------- 2. UI：悬浮球 + 管理面板 ----------

    function mount() {
        const host0 = document.createElement('div');
        host0.id = 'rd-x9k2-host';
        const shadow = host0.attachShadow({ mode: 'open' });
        const style = document.createElement('style');
        style.textContent = `
            :host { all: initial; }
            .ball { position: fixed; right: 24px; bottom: 24px; z-index: 2147483647;
                    width: 52px; height: 52px; border-radius: 50%;
                    background: linear-gradient(135deg, #4f8cff, #2b5fd9);
                    color: #fff; font: 24px/52px sans-serif; text-align: center;
                    cursor: pointer; user-select: none;
                    box-shadow: 0 4px 16px rgba(0,0,0,.4); opacity: .85;
                    transition: opacity .2s, transform .2s;
                    display: block !important; visibility: visible !important; }
            .ball.has { background: linear-gradient(135deg, #66bb6a, #2e7d32); }
            .badge { position: fixed; right: 16px; bottom: 64px; z-index: 2147483647;
                     min-width: 20px; height: 20px; border-radius: 10px; padding: 0 5px;
                     background: #e53935; color: #fff; font: bold 12px/20px sans-serif;
                     text-align: center; box-shadow: 0 2px 8px rgba(0,0,0,.4);
                     display: block !important; visibility: visible !important; }
            .tip { position: fixed; right: 24px; bottom: 96px; z-index: 2147483647;
                   max-width: 340px; padding: 8px 12px; border-radius: 8px;
                   background: rgba(0,0,0,.85); color: #fff; font: 13px/1.5 sans-serif;
                   display: none; box-shadow: 0 4px 16px rgba(0,0,0,.4); word-break: break-all; }
            .panel { position: fixed; top: 50%; left: 50%; transform: translate(-50%,-50%);
                     width: 460px; max-width: 92vw; max-height: 78vh; z-index: 2147483647;
                     background: #fff; color: #222; border-radius: 12px;
                     box-shadow: 0 12px 48px rgba(0,0,0,.35);
                     font: 13px/1.5 -apple-system, sans-serif;
                     display: none; flex-direction: column; overflow: hidden; }
            .panel.open { display: flex; }
            .p-head { padding: 12px 16px; background: #2b5fd9; color: #fff;
                      display: flex; align-items: center; gap: 8px; }
            .p-head .title { font-weight: 600; flex: 1; }
            .p-head .close { cursor: pointer; opacity: .85; font-size: 18px; line-height: 1; padding: 2px 6px; }
            .p-head .close:hover { opacity: 1; }
            .p-status { padding: 6px 16px; font-size: 12px; color: #666;
                        border-bottom: 1px solid #eee; display: flex; gap: 6px; align-items: center; }
            .dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
            .dot.ok { background: #43a047; } .dot.bad { background: #e53935; }
            .p-list { flex: 1; overflow-y: auto; padding: 4px 0; }
            .item { display: flex; align-items: center; gap: 8px; padding: 8px 16px; border-bottom: 1px solid #f2f2f2; }
            .item:hover { background: #f7f9ff; }
            .item input[type=checkbox] { flex: none; }
            .item .info { flex: 1; min-width: 0; }
            .item .t { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
            .item .meta { font-size: 11px; color: #888; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
            .item .st { flex: none; font-size: 11px; padding: 2px 8px; border-radius: 10px; }
            .st.pending { background: #fff3e0; color: #e65100; }
            .st.sending { background: #e3f2fd; color: #1565c0; }
            .st.sent { background: #e8f5e9; color: #2e7d32; }
            .st.failed { background: #ffebee; color: #b71c1c; }
            .item .del { flex: none; cursor: pointer; color: #bbb; font-size: 15px; padding: 0 4px; }
            .item .del:hover { color: #e53935; }
            .p-empty { padding: 32px 16px; text-align: center; color: #999; }
            .p-foot { padding: 10px 16px; border-top: 1px solid #eee; display: flex; gap: 8px; align-items: center; }
            .p-foot .grow { flex: 1; }
            button { font: 12px/1 -apple-system, sans-serif; padding: 7px 12px; border-radius: 6px;
                     border: 1px solid #ddd; background: #fff; cursor: pointer; color: #333; }
            button:hover { background: #f5f5f5; }
            button.primary { background: #2b5fd9; border-color: #2b5fd9; color: #fff; }
            button.primary:hover { background: #2450bd; }
            button.primary:disabled { background: #a9c0ee; border-color: #a9c0ee; cursor: not-allowed; }
            button.danger { color: #b71c1c; border-color: #ef9a9a; }
            button.danger:hover { background: #ffebee; }
        `;
        const ball = document.createElement('div');
        ball.className = 'ball';
        ball.title = '下载管理（点击打开）';
        ball.textContent = '↓';
        const badge = document.createElement('div');
        badge.className = 'badge';
        badge.style.display = 'none';
        const tip = document.createElement('div');
        tip.className = 'tip';
        const panel = document.createElement('div');
        panel.className = 'panel';
        shadow.append(style, ball, badge, tip, panel);
        ballEl = ball;
        badgeEl = badge;

        function showTip(text, color) {
            tip.textContent = text;
            tip.style.borderLeft = `4px solid ${color || '#4f8cff'}`;
            tip.style.display = 'block';
            clearTimeout(showTip._t);
            showTip._t = setTimeout(() => (tip.style.display = 'none'), 5000);
        }

        // ---- 角标 ----
        function refreshBadge() {
            const n = IS_TOP ? pendingCount() : 0;
            if (n > 0) {
                badge.textContent = String(n);
                badge.style.display = 'block';
                ball.classList.add('has');
            } else {
                badge.style.display = 'none';
                ball.classList.remove('has');
            }
        }
        onTasksChanged = refreshBadge;

        // ---- 管理面板 ----
        let selected = new Set();
        let sending = false;

        function fmtTime(ts) {
            const d = new Date(ts);
            const p = (x) => String(x).padStart(2, '0');
            return `${p(d.getHours())}:${p(d.getMinutes())}`;
        }

        function esc(s) {
            return String(s).replace(/[&<>"']/g, (c) =>
                ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
        }

        function renderPanel() {
            const tasks = loadTasks();
            // 清理已不存在的选中项
            const ids = new Set(tasks.map((t) => t.id));
            selected = new Set([...selected].filter((i) => ids.has(i)));
            const stText = { pending: '待下载', sending: '发送中', sent: '已入队', failed: '失败' };
            const rows = tasks.map((t) => `
                <div class="item" data-id="${t.id}">
                    <input type="checkbox" ${selected.has(t.id) ? 'checked' : ''}>
                    <div class="info">
                        <div class="t" title="${esc(t.url)}">${esc(t.title)}</div>
                        <div class="meta">${esc(t.host)} · ${fmtTime(t.t)}${t.err ? ' · ' + esc(t.err.slice(0, 60)) : ''}</div>
                    </div>
                    <span class="st ${t.status}">${stText[t.status] || t.status}</span>
                    <span class="del" title="删除">✕</span>
                </div>`);
            panel.innerHTML = `
                <div class="p-head">
                    <span class="title">下载任务（${tasks.length}）</span>
                    <span class="close" title="关闭">✕</span>
                </div>
                <div class="p-status" id="rd-status">
                    <span class="dot" id="rd-dot"></span><span id="rd-srv">检测本地服务…</span>
                    <span style="margin-left:auto" id="rd-cnt"></span>
                </div>
                <div class="p-list">${rows.length ? rows.join('') : '<div class="p-empty">暂无任务<br>打开播放页让视频播放 2-3 秒，会自动捕获暂存</div>'}</div>
                <div class="p-foot">
                    <button id="rd-sel">全选</button>
                    <button id="rd-clr" class="danger">清除已结束</button>
                    <span class="grow"></span>
                    <button id="rd-dl" class="primary" ${sending ? 'disabled' : ''}>下载选中 (${selected.size})</button>
                </div>`;
            bindPanel();
            checkServer();
        }

        function bindPanel() {
            panel.querySelector('.close').onclick = () => panel.classList.remove('open');
            panel.querySelectorAll('.item').forEach((el) => {
                const id = el.dataset.id;
                el.querySelector('input[type=checkbox]').onchange = (e) => {
                    if (e.target.checked) selected.add(id); else selected.delete(id);
                    panel.querySelector('#rd-dl').textContent = `下载选中 (${selected.size})`;
                };
                el.querySelector('.del').onclick = () => { removeTasks([id]); selected.delete(id); renderPanel(); };
            });
            panel.querySelector('#rd-sel').onclick = () => {
                const all = loadTasks();
                if (selected.size >= all.length) selected = new Set();
                else all.forEach((t) => selected.add(t.id));
                renderPanel();
            };
            panel.querySelector('#rd-clr').onclick = () => {
                removeTasks(loadTasks().filter((t) => t.status === 'sent' || t.status === 'failed').map((t) => t.id));
                renderPanel();
            };
            panel.querySelector('#rd-dl').onclick = () => batchSend();
        }

        function checkServer() {
            const dot = panel.querySelector('#rd-dot');
            const txt = panel.querySelector('#rd-srv');
            GM_xmlhttpRequest({
                method: 'GET', url: SERVER + '/health', timeout: 2500,
                onload: () => { if (dot) { dot.className = 'dot ok'; txt.textContent = '本地服务运行中 · ' + SERVER; } },
                onerror: () => { if (dot) { dot.className = 'dot bad'; txt.textContent = '本地服务未启动：./target/release/video_processor --server'; } },
                ontimeout: () => { if (dot) { dot.className = 'dot bad'; txt.textContent = '本地服务响应超时'; } },
            });
        }

        function sendOne(t) {
            return new Promise((resolve) => {
                GM_xmlhttpRequest({
                    method: 'POST',
                    url: SERVER + '/download',
                    headers: { 'Content-Type': 'application/json' },
                    data: JSON.stringify({ url: t.url }),
                    timeout: 30000,
                    onload: (resp) => {
                        let data = {};
                        try { data = JSON.parse(resp.responseText); } catch (e) { /* 忽略 */ }
                        if (resp.status === 200 && data.ok) resolve({ ok: true, msg: `${data.segments} 个分片` });
                        else resolve({ ok: false, msg: data.error || 'HTTP ' + resp.status });
                    },
                    onerror: () => resolve({ ok: false, msg: '连不上本地服务' }),
                    ontimeout: () => resolve({ ok: false, msg: '响应超时' }),
                });
            });
        }

        async function batchSend() {
            if (sending || selected.size === 0) return;
            sending = true;
            const btn = panel.querySelector('#rd-dl');
            btn.disabled = true;
            const ids = [...selected];
            let okCount = 0, failCount = 0;
            for (const id of ids) {
                const t = loadTasks().find((x) => x.id === id);
                if (!t) continue;
                updateTask(id, { status: 'sending' });
                renderPanel();
                const r = await sendOne(t);
                if (r.ok) { okCount++; updateTask(id, { status: 'sent', err: '' }); selected.delete(id); }
                else { failCount++; updateTask(id, { status: 'failed', err: r.msg }); }
                renderPanel();
            }
            sending = false;
            showTip(`批量发送完成：${okCount} 成功${failCount ? '，' + failCount + ' 失败（详情见面板）' : ''}`,
                failCount ? '#ffb74d' : '#81c784');
        }

        // ---- 拖动 + 点击 ----
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

        ball.addEventListener('mouseenter', () => {
            ball.style.opacity = '1';
            const n = pendingCount();
            if (n > 0) showTip(`有 ${n} 个待下载任务，点击打开管理面板`, '#81c784');
        });
        ball.addEventListener('mouseleave', () => (ball.style.opacity = '0.85'));

        ball.addEventListener('click', () => {
            if (moved) return; // 拖动结束不触发
            const opening = !panel.classList.contains('open');
            panel.classList.toggle('open');
            if (opening) renderPanel();
        });

        function mountBall() {
            if (document.getElementById('rd-x9k2-host')) return true;
            if (!document.documentElement) return false;
            // 挂 <html>（防 SPA 重建 body 删除）+ Shadow DOM（防广告拦截器 cosmetic 规则）
            document.documentElement.append(host0);
            console.log('[rou-dl] 悬浮球已挂载 @', location.href);
            return true;
        }
        if (!mountBall()) {
            document.addEventListener('DOMContentLoaded', mountBall);
            window.addEventListener('load', mountBall);
        }

        // 防移除观察器：SPA 重渲染后自动补挂
        const observer = new MutationObserver(() => {
            if (!document.getElementById('rd-x9k2-host')) mountBall();
        });
        const startObserver = () => observer.observe(document.documentElement, { childList: true, subtree: false });
        if (document.documentElement) startObserver();
        else document.addEventListener('DOMContentLoaded', startObserver, { once: true });

        // 角标轮询：其他页面/frame 写入 GM 存储后这里也能看到（GM 存储全局共享）
        setInterval(refreshBadge, 2000);
        refreshBadge();
        window.addEventListener('load', () => { scanPageSource(); refreshBadge(); });
    }

    // ---------- 3. 启动 ----------

    try {
        // 接收 iframe 捕获通知（仅用于立即刷新角标；数据本身经 GM 存储共享）
        window.addEventListener('message', (e) => {
            const d = e.data;
            if (d && typeof d === 'object' && d.__rdCapture) {
                notifyTasksChanged();
            }
        });

        if (IS_TOP) {
            mount(); // 悬浮球只挂在顶层；iframe 实例只负责捕获（写 GM 存储）
        } else {
            // iframe 实例：load 后也扫一次源码/网络记录
            window.addEventListener('load', scanPageSource);
        }

        // load 后 3s 自动 API 重放（不依赖用户操作）
        window.addEventListener('load', () => {
            setTimeout(() => captureFromApiReplay().then(refreshAll), 3000);
        });
        function refreshAll() { notifyTasksChanged(); }
    } catch (e) {
        console.error('[rou-dl] 悬浮球初始化失败:', e);
    }
})();
