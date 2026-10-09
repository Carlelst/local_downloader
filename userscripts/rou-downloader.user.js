// ==UserScript==
// @name         rou.video 悬浮球下载器
// @namespace    https://rou.video/
// @version      2.2.0
// @description  在 rou.video / missav 播放页右下角显示悬浮球：自动捕获视频地址并暂存为任务（跨页面持久化），点击球打开管理面板：批量发送下载、实时查看服务端进度、取消/重试、手动添加地址
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
            const list = Array.isArray(arr) ? arr : [];
            // 一次性迁移：旧版按完整 URL 去重，auth/exp 轮换把同一视频存成了多条 →
            // 按视频稳定键合并保留最新一条，并把 id 重算为新键（只执行一次）
            if (!GM_getValue(TASKS_KEY + '_mig2', false)) {
                GM_setValue(TASKS_KEY + '_mig2', true);
                const byKey = new Map();
                for (const t of list) {
                    const k = videoKey(t.url);
                    const prev = byKey.get(k);
                    if (!prev || (t.t || 0) > (prev.t || 0)) {
                        byKey.set(k, Object.assign({}, t, { id: taskId(k) }));
                    }
                }
                const merged = [...byKey.values()];
                saveTasks(merged);
                return merged;
            }
            return list;
        } catch (e) { return []; }
    }
    function saveTasks(tasks) {
        try { GM_setValue(TASKS_KEY, JSON.stringify(tasks)); } catch (e) { /* 忽略 */ }
    }
    // 视频稳定键：host + 路径（去掉 query 与 index/playlist 文件名）。
    // 同一视频的 auth/exp 轮换只改 query，重复捕获与 /status 匹配都按此键归一
    // （与服务端 server.rs 的 video_key 逻辑保持一致）
    function videoKey(url) {
        try {
            const u = new URL(url);
            return u.host + u.pathname.replace(/\/(index|playlist)\.(png|jpg|jpeg|webp|m3u8)$/i, '');
        } catch (e) { return url.split('?')[0]; }
    }
    function taskId(url) {
        // 入参为 videoKey：同一视频同一任务，用稳定哈希做 id
        let h = 0;
        for (let i = 0; i < url.length; i++) { h = (h * 31 + url.charCodeAt(i)) | 0; }
        return 't' + (h >>> 0).toString(36);
    }
    // 从 URL 提取语义标题（页面 title 常是站点通用名，URL 里的视频 id 更有区分度）
    function titleFromUrl(url) {
        try {
            const u = new URL(url);
            const segs = u.pathname.split('/').filter(Boolean);
            const skip = /^(index|playlist)\.(png|jpg|jpeg|webp|m3u8)$/i;
            const seg = [...segs].reverse().find((s) => !skip.test(s));
            const t = seg || u.host;
            return t.length > 80 ? t.slice(0, 80) : t;
        } catch (e) { return url.slice(0, 60); }
    }
    // 播放页标题（含视频名）：去掉站点后缀（"xxx - missav" 之类），用作输出文件名
    function pageTitle() {
        try {
            let t = (document.title || '').trim();
            t = t.replace(/\s*[-|–]\s*(rou\.video|missav[\w.]*|www\.[\w.]+)\s*$/i, '').trim();
            return t.length > 120 ? t.slice(0, 120) : t;
        } catch (e) { return ''; }
    }
    /** 捕获入队：按视频稳定键去重（auth/exp 轮换不产生新条目）。
     *  返回 'new' 新增 / 'refresh' 刷新了已有条目的地址 / false 无变化 */
    function addTask(url, title) {
        const tasks = loadTasks();
        const key = videoKey(url);
        const id = taskId(key);
        const existing = tasks.find((t) => t.id === id);
        if (existing) {
            // 同一视频重复捕获：刷新为最新 auth 地址（旧地址会过期）；页面标题可用时一并更新
            if (existing.url !== url) {
                existing.url = url;
                existing.t = Date.now();
                if (title && title.length > 3 && title !== existing.title) existing.title = title;
                saveTasks(tasks);
                return 'refresh';
            }
            return false;
        }
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
        return 'new';
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
            const title = pageTitle() || titleFromUrl(clean);
            const r = addTask(clean, title);
            if (r === 'new') {
                console.log('[rou-dl] ✓ 已捕获并暂存 (' + source + '):', clean);
                notifyTasksChanged();
                reportToTop(clean);
            } else if (r === 'refresh') {
                // 同一视频换了新 auth 地址：静默刷新存储，面板打开时自动重绘
                notifyTasksChanged();
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
                    // Response.url 是 302 重定向后的最终地址：rou.video 的 API 响应一返回
                    // 就能拿到带 auth 的播放 URL（无需等重放轮询，实现点击播放即捕获）
                    if (resp.url) capture(resp.url, 'fetch-最终地址');
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
                    // responseURL 同 fetch 的 resp.url：302 链最终地址，立即捕获
                    capture(this.responseURL, 'xhr-最终地址');
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
    // 持续扫描不停止：SPA 站内导航时 load 只触发一次，后续视频只能靠轮询兜底
    setInterval(() => scanPerformance(), 2000);

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
                     width: 460px; min-width: 340px; max-width: 92vw; max-height: 78vh;
                     min-height: 220px; z-index: 2147483647;
                     background: #fff; color: #222; border-radius: 12px;
                     box-shadow: 0 12px 48px rgba(0,0,0,.35);
                     font: 13px/1.5 -apple-system, sans-serif;
                     display: none; flex-direction: column; overflow: hidden; resize: both; }
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
            .p-list { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0; }
            .p-list::-webkit-scrollbar { width: 8px; }
            .p-list::-webkit-scrollbar-track { background: #f5f6f8; }
            .p-list::-webkit-scrollbar-thumb { background: #c5cad3; border-radius: 4px; }
            .p-list::-webkit-scrollbar-thumb:hover { background: #a8b0bd; }
            .p-tabs { display: flex; gap: 6px; padding: 8px 16px 0; }
            .ptab { font-size: 12px; padding: 4px 12px; border-radius: 12px; cursor: pointer;
                    background: #f0f2f5; color: #555; user-select: none; }
            .ptab:hover { background: #e3e7ee; }
            .ptab.on { background: #2b5fd9; color: #fff; }
            .pg { height: 4px; margin-top: 4px; background: #e8eaf0; border-radius: 2px; overflow: hidden; }
            .pg i { display: block; height: 100%; border-radius: 2px; background: linear-gradient(90deg, #4f8cff, #2b5fd9); transition: width .4s; }
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
            .item.srv { background: #fafbff; }
            .act { flex: none; cursor: pointer; font-size: 11px; padding: 2px 8px; border-radius: 10px;
                   background: #e3f2fd; color: #1565c0; }
            .act:hover { background: #bbdefb; }
            .p-add { display: flex; gap: 8px; padding: 10px 16px 0; }
            .p-add input { flex: 1; min-width: 0; font: 12px/1.4 -apple-system, sans-serif; padding: 7px 10px;
                           border: 1px solid #ddd; border-radius: 6px; outline: none; }
            .p-add input:focus { border-color: #2b5fd9; }
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
        // 任务变动（捕获/刷新）：刷新角标；面板已打开时同步重绘（捕获后自动出现，无需手点）
        onTasksChanged = () => {
            refreshBadge();
            const input = panel.querySelector('#rd-url');
            if (panel.classList.contains('open') && shadow.activeElement !== input) renderPanel();
        };

        // ---- 管理面板 ----
        let selected = new Set();
        let sending = false;
        // 当前 tab：pending 待下载 / active 进行中 / done 已结束
        let curTab = 'active';
        // 服务端任务状态（GET /status 轮询）：url -> {state,segments,error,name}
        let serverTasks = {};
        let serverCounts = null;
        let lastStatusJson = '';
        const SRV_ST = {
            queued: ['排队中', 'sending'],
            downloading: ['下载中', 'sending'],
            cancelling: ['取消中', 'failed'],
            done: ['完成', 'sent'],
            failed: ['失败', 'failed'],
            cancelled: ['已取消', 'failed'],
        };

        function fmtTime(ts) {
            const d = new Date(ts);
            const p = (x) => String(x).padStart(2, '0');
            return `${p(d.getHours())}:${p(d.getMinutes())}`;
        }

        function esc(s) {
            return String(s).replace(/[&<>"']/g, (c) =>
                ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
        }

        // 分片进度条：排队（0%）与下载中实时显示（segments_done 由服务端 /status 注入）
        function pgBar(sv) {
            if (!sv || !sv.segments) return '';
            if (sv.state !== 'downloading' && sv.state !== 'queued') return '';
            const dn = sv.segments_done || 0;
            const pct = Math.min(100, Math.round((dn / sv.segments) * 100));
            return `<div class="pg"><i style="width:${pct}%"></i></div>`;
        }

        function renderPanel() {
            const tasks = loadTasks();
            // 清理已不存在的选中项
            const ids = new Set(tasks.map((t) => t.id));
            selected = new Set([...selected].filter((i) => ids.has(i)));
            const stText = { pending: '待下载', sending: '发送中', sent: '已入队', failed: '失败' };
            const ACTIVE_SRV = ['queued', 'downloading', 'cancelling'];
            const rows = tasks.map((t) => {
                // 已入队任务合并显示服务端实时状态（按视频稳定键匹配，auth 轮换不影响）
                const sv = t.status === 'sent' ? serverTasks[videoKey(t.url)] : null;
                let chip = `<span class="st ${t.status}">${stText[t.status] || t.status}</span>`;
                let act = '';
                let metaExtra = t.err ? ' · ' + esc(t.err.slice(0, 60)) : '';
                if (sv) {
                    const [lbl, cls] = SRV_ST[sv.state] || [sv.state, 'sending'];
                    chip = `<span class="st ${cls}">${lbl}</span>`;
                    if (sv.segments) {
                        const dn = sv.segments_done || 0;
                        metaExtra = (sv.state === 'downloading' ? ` · ${dn}/${sv.segments} 片` : ' · ' + sv.segments + ' 分片') + metaExtra;
                    }
                    if (ACTIVE_SRV.includes(sv.state)) {
                        act = '<span class="act" data-act="cancel">取消</span>';
                    } else if (sv.state === 'failed' || sv.state === 'cancelled') {
                        act = '<span class="act" data-act="retry">重试</span>';
                        if (sv.error) metaExtra = ' · ' + esc(sv.error.slice(0, 80));
                    } else if (sv.state === 'done') {
                        // 已完成也可重新下载（服务端 done 再提交=重新入队，输出为新文件名不覆盖旧文件）
                        act = '<span class="act" data-act="retry">重下</span>';
                    }
                } else if (t.status === 'failed') {
                    // GM 侧发送失败：点击重置为待下载，可再次批量发送
                    act = '<span class="act" data-act="resend">重试</span>';
                }
                // 行归属 tab：服务端终态→已结束；已发送未结束→进行中；未发送→待下载
                const END_SRV = ['done', 'failed', 'cancelled'];
                const bucket = sv && END_SRV.includes(sv.state) ? 'done' : (t.status === 'sent' ? 'active' : 'pending');
                return { bucket, html: `
                <div class="item" data-id="${t.id}" data-url="${esc(t.url)}">
                    <input type="checkbox" ${selected.has(t.id) ? 'checked' : ''}>
                    <div class="info">
                        <div class="t" title="${esc(t.url)}">${esc(t.title)}</div>
                        <div class="meta">${esc(t.host)} · ${fmtTime(t.t)}${metaExtra}</div>
                        ${pgBar(sv)}
                    </div>
                    ${chip}
                    ${act}
                    <span class="del" title="删除">✕</span>
                </div>` };
            });
            // 服务端有、本列表没有的任务（服务重启前的残留 / 其他页面发送的）
            const srvOnly = Object.values(serverTasks)
                .filter((sv) => !ids.has(taskId(videoKey(sv.url))))
                .map((sv) => {
                    const [lbl, cls] = SRV_ST[sv.state] || [sv.state, 'sending'];
                    let host = '';
                    try { host = new URL(sv.url).host; } catch (e) { /* 忽略 */ }
                    let segTxt = '';
                    if (sv.segments) {
                        const dn = sv.segments_done || 0;
                        segTxt = sv.state === 'downloading' ? ` · ${dn}/${sv.segments} 片` : ' · ' + sv.segments + ' 分片';
                    }
                    const act = ACTIVE_SRV.includes(sv.state)
                        ? '<span class="act" data-act="cancel">取消</span>'
                        : ((sv.state === 'failed' || sv.state === 'cancelled')
                            ? '<span class="act" data-act="retry">重试</span>'
                            : (sv.state === 'done' ? '<span class="act" data-act="retry">重下</span>' : ''));
                    const del = ACTIVE_SRV.includes(sv.state)
                        ? ''
                        : '<span class="del" data-del="1" title="从列表移除">✕</span>';
                    const bucket = ['done', 'failed', 'cancelled'].includes(sv.state) ? 'done' : 'active';
                    return { bucket, html: `
                <div class="item srv" data-url="${esc(sv.url)}">
                    <input type="checkbox" disabled>
                    <div class="info">
                        <div class="t" title="${esc(sv.url)}">${esc(sv.name || sv.url.slice(0, 60))}</div>
                        <div class="meta">${esc(host)}${segTxt}${sv.error ? ' · ' + esc(sv.error.slice(0, 60)) : ''}</div>
                        ${pgBar(sv)}
                    </div>
                    <span class="st ${cls}">${lbl}</span>
                    ${act}
                    ${del}
                </div>` };
                });
            const c = serverCounts;
            const cnt = c
                ? `排队 ${c.queued || 0} · 下载 ${(c.downloading || 0) + (c.cancelling || 0)} · 完成 ${c.done || 0} · 失败 ${(c.failed || 0) + (c.cancelled || 0)}`
                : '';
            const all = rows.concat(srvOnly);
            // 按当前 tab 过滤 + 计数（待下载=未发送，进行中=已发送未结束，已结束=服务端终态）
            const counts = {
                pending: all.filter((r) => r.bucket === 'pending').length,
                active: all.filter((r) => r.bucket === 'active').length,
                done: all.filter((r) => r.bucket === 'done').length,
            };
            const shown = all.filter((r) => r.bucket === curTab).map((r) => r.html);
            const emptyText = {
                pending: '暂无待下载任务<br>播放视频会自动捕获暂存，也可在下方手动粘贴',
                active: '暂无进行中的任务<br>切到"待下载"勾选任务后点"下载选中"',
                done: '暂无已结束的任务',
            }[curTab];
            panel.innerHTML = `
                <div class="p-head">
                    <span class="title">下载任务（${all.length}）</span>
                    <span class="close" title="关闭">✕</span>
                </div>
                <div class="p-status" id="rd-status">
                    <span class="dot" id="rd-dot"></span><span id="rd-srv">检测本地服务…</span>
                    <span style="margin-left:auto" id="rd-cnt">${esc(cnt)}</span>
                </div>
                <div class="p-tabs">
                    <span class="ptab ${curTab === 'pending' ? 'on' : ''}" data-tab="pending">待下载 ${counts.pending}</span>
                    <span class="ptab ${curTab === 'active' ? 'on' : ''}" data-tab="active">进行中 ${counts.active}</span>
                    <span class="ptab ${curTab === 'done' ? 'on' : ''}" data-tab="done">已结束 ${counts.done}</span>
                </div>
                <div class="p-list">${shown.length ? shown.join('') : `<div class="p-empty">${emptyText}</div>`}</div>
                <div class="p-add">
                    <input id="rd-url" type="text" placeholder="手动粘贴视频地址（index.png / .m3u8，含 auth 参数）">
                    <button id="rd-add">添加</button>
                </div>
                <div class="p-foot">
                    <button id="rd-sel">全选</button>
                    <button id="rd-scan" title="立刻重放 API / 扫描网络记录与源码">立即捕获</button>
                    <button id="rd-clr" class="danger">清除已结束</button>
                    <span class="grow"></span>
                    <button id="rd-dl" class="primary" ${sending ? 'disabled' : ''}>下载选中 (${selected.size})</button>
                </div>`;
            bindPanel();
            checkServer();
        }

        // 面板动作：取消 / 重试（服务端重下）/ 重发（GM 侧失败重置）/ 移除（服务端列表）
        async function doAction(act, url) {
            if (!url) return;
            if (act === 'cancel') {
                GM_xmlhttpRequest({
                    method: 'POST', url: SERVER + '/cancel', timeout: 5000,
                    headers: { 'Content-Type': 'application/json' },
                    data: JSON.stringify({ url }),
                    onload: () => { pollStatus(); showTip('已请求取消，下载中任务会在分片间隙停止并清理缓存', '#ffb74d'); },
                    onerror: () => showTip('取消失败：本地服务未响应', '#ffb74d'),
                    ontimeout: () => showTip('取消请求超时', '#ffb74d'),
                });
            } else if (act === 'retry') {
                // 重试/重下也带上 GM 存储里的视频标题（地址已刷新但标题不变）
                const gm = loadTasks().find((t) => t.id === taskId(videoKey(url)));
                const r = await sendOne({ url, name: gm ? gm.title : '' });
                showTip(r.ok ? '已重新入队下载' : '重试失败：' + r.msg, r.ok ? '#81c784' : '#ffb74d');
                pollStatus();
            } else if (act === 'resend') {
                updateTask(taskId(videoKey(url)), { status: 'pending', err: '' });
                renderPanel();
            } else if (act === 'forget') {
                GM_xmlhttpRequest({
                    method: 'POST', url: SERVER + '/forget', timeout: 5000,
                    headers: { 'Content-Type': 'application/json' },
                    data: JSON.stringify({ url }),
                    onload: () => pollStatus(),
                    onerror: () => {},
                });
            }
        }

        function addManually() {
            const inp = panel.querySelector('#rd-url');
            const raw = (inp.value || '').trim();
            if (!raw) return;
            const url = raw.split('#')[0];
            if (!/^https?:\/\//i.test(url)) { showTip('请输入 http(s):// 开头的完整地址', '#ffb74d'); return; }
            const r = addTask(url, titleFromUrl(url));
            if (r) {
                inp.value = '';
                notifyTasksChanged();
                renderPanel();
                showTip(r === 'new' ? '已添加，勾选后点"下载选中"' : '该视频已在列表中，已刷新为最新地址', '#81c784');
            } else {
                showTip('该地址已在任务列表中', '#ffb74d');
            }
        }

        function bindPanel() {
            panel.querySelector('.close').onclick = () => panel.classList.remove('open');
            // tab 切换（待下载/进行中/已结束）
            panel.querySelectorAll('.ptab').forEach((el) => {
                el.onclick = () => { curTab = el.dataset.tab; renderPanel(); };
            });
            panel.querySelectorAll('.item').forEach((el) => {
                const id = el.dataset.id;
                const url = el.dataset.url;
                const cb = el.querySelector('input[type=checkbox]');
                if (cb && !cb.disabled) {
                    cb.onchange = (e) => {
                        if (e.target.checked) selected.add(id); else selected.delete(id);
                        panel.querySelector('#rd-dl').textContent = `下载选中 (${selected.size})`;
                    };
                    el.querySelector('.del').onclick = () => { removeTasks([id]); selected.delete(id); renderPanel(); };
                } else {
                    // 服务端任务行：✕ = 从服务端列表移除
                    const d = el.querySelector('.del');
                    if (d) d.onclick = () => doAction('forget', url);
                }
                el.querySelectorAll('.act').forEach((a) => {
                    a.onclick = () => doAction(a.dataset.act, url);
                });
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
            panel.querySelector('#rd-add').onclick = addManually;
            panel.querySelector('#rd-url').onkeydown = (e) => { if (e.key === 'Enter') addManually(); };
            panel.querySelector('#rd-scan').onclick = async () => {
                const b = panel.querySelector('#rd-scan');
                b.disabled = true; b.textContent = '扫描中…';
                const before = loadTasks().length;
                scanPageSource();
                scanPerformance();
                await captureFromApiReplay();
                renderPanel();
                const added = loadTasks().length > before;
                showTip(added ? '捕获到新视频 ✓' : '未捕获到：把视频多播几秒再点一次，或在下方手动粘贴地址', added ? '#81c784' : '#ffb74d');
            };
        }

        // ---- 服务端任务状态轮询（GET /status，2s）----
        function pollStatus() {
            GM_xmlhttpRequest({
                method: 'GET', url: SERVER + '/status', timeout: 2500,
                onload: (resp) => {
                    try {
                        const d = JSON.parse(resp.responseText);
                        if (d && d.ok) {
                            const map = {};
                            (d.tasks || []).forEach((t) => { map[videoKey(t.url)] = t; });
                            serverTasks = map;
                            serverCounts = d.counts || {};
                            const snap = JSON.stringify([serverTasks, serverCounts]);
                            if (snap !== lastStatusJson) {
                                lastStatusJson = snap;
                                refreshBadge();
                                // 面板打开且未在输入框打字时才重绘（避免打断输入）
                                const input = panel.querySelector('#rd-url');
                                if (panel.classList.contains('open') && shadow.activeElement !== input) renderPanel();
                            }
                        }
                    } catch (e) { /* 忽略 */ }
                },
                onerror: () => {
                    if (serverCounts) {
                        serverTasks = {}; serverCounts = null; lastStatusJson = '';
                        if (panel.classList.contains('open')) renderPanel();
                    }
                },
            });
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
                    // name = 视频标题：服务端用它做输出文件名（清洗非法字符+防撞哈希）
                    data: JSON.stringify({ url: t.url, name: t.name || t.title || '' }),
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
        // 服务端任务状态轮询（面板进度 / 取消 / 重试的数据源）
        pollStatus();
        setInterval(pollStatus, 2000);
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

        // load+3s 首次 API 重放，之后周期重试（SPA 站内导航不会重触发 load，
        // 且 API 调用可能发生在点击播放之后——周期重试保证最终能捕获）。
        // 后台标签页跳过重放，避免无谓请求；约 30 分钟后自动停止。
        const REPLAY_INTERVAL = 10000;
        const REPLAY_MAX_TRIES = 180;
        let replayTries = 0;
        let replayTimer = null;
        function replayOnce() {
            if (document.visibilityState !== 'visible') return;
            replayTries++;
            if (replayTries > REPLAY_MAX_TRIES) {
                if (replayTimer) { clearInterval(replayTimer); replayTimer = null; }
                return;
            }
            captureFromApiReplay().then((ok) => { if (ok) notifyTasksChanged(); });
        }
        window.addEventListener('load', () => {
            setTimeout(replayOnce, 3000);
            replayTimer = setInterval(replayOnce, REPLAY_INTERVAL);
        });
    } catch (e) {
        console.error('[rou-dl] 悬浮球初始化失败:', e);
    }
})();
