const GLOBAL_KEY = '__TAURITAVERN_PERF__';
const STORAGE_ENABLED_KEY = 'tt:perf';
const STORAGE_POSITION_KEY = 'tt:perf:pos';

const DEFAULTS = Object.freeze({
    updateIntervalMs: 750,
    frameSampleSize: 60,
    longFrameThresholdMs: 50,
    maxInvokeStats: 30,
    maxRecentLongFrames: 200,
    maxRecentLongTasks: 200,
    maxRecentInvokes: 200,
});

function safeNow() {
    try {
        return globalThis.performance?.now?.() ?? Date.now();
    } catch {
        return Date.now();
    }
}

function safeTimeOrigin() {
    try {
        return globalThis.performance?.timeOrigin ?? 0;
    } catch {
        return 0;
    }
}

function safeMark(name, detail) {
    try {
        globalThis.performance?.mark?.(name, detail ? { detail } : undefined);
    } catch {
        // Ignore unsupported mark calls.
    }
}

function safeMeasure(name, startMark, endMark) {
    try {
        globalThis.performance?.measure?.(name, startMark, endMark);
    } catch {
        // Ignore unsupported measure calls.
    }
}

function readAutoEnableFlag() {
    try {
        if (globalThis.localStorage?.getItem(STORAGE_ENABLED_KEY) === '1') {
            return true;
        }
    } catch {
        // Ignore storage access failure.
    }

    try {
        const search = String(globalThis.location?.search || '');
        if (!search) {
            return false;
        }
        const params = new URLSearchParams(search);
        return params.get('ttPerf') === '1' || params.get('tt_perf') === '1';
    } catch {
        return false;
    }
}

function persistEnabledFlag(enabled) {
    try {
        globalThis.localStorage?.setItem(STORAGE_ENABLED_KEY, enabled ? '1' : '0');
    } catch {
        // Ignore storage write failure.
    }
}

function readStoredPosition() {
    try {
        const raw = globalThis.localStorage?.getItem(STORAGE_POSITION_KEY);
        if (!raw) {
            return null;
        }

        const parsed = JSON.parse(raw);
        const x = Number(parsed?.x);
        const y = Number(parsed?.y);
        if (!Number.isFinite(x) || !Number.isFinite(y)) {
            return null;
        }

        return { x, y };
    } catch {
        return null;
    }
}

function persistPosition(pos) {
    try {
        if (!pos || typeof pos !== 'object') {
            globalThis.localStorage?.removeItem(STORAGE_POSITION_KEY);
            return;
        }
        const x = Number(pos.x);
        const y = Number(pos.y);
        if (!Number.isFinite(x) || !Number.isFinite(y)) {
            return;
        }
        globalThis.localStorage?.setItem(STORAGE_POSITION_KEY, JSON.stringify({ x, y }));
    } catch {
        // Ignore storage write failure.
    }
}

function clampInt(value, min, max, fallback) {
    const n = Number(value);
    if (!Number.isFinite(n)) {
        return fallback;
    }
    const i = Math.trunc(n);
    if (i < min) {
        return min;
    }
    if (i > max) {
        return max;
    }
    return i;
}

function formatMs(ms) {
    const n = Number(ms);
    if (!Number.isFinite(n)) {
        return '-';
    }
    if (n < 1000) {
        return `${Math.round(n)}ms`;
    }
    return `${(n / 1000).toFixed(2)}s`;
}

function formatMB(bytes) {
    const n = Number(bytes);
    if (!Number.isFinite(n)) {
        return '-';
    }
    return `${(n / (1024 * 1024)).toFixed(1)}MB`;
}

function createPerfController(options = {}) {
    const config = {
        updateIntervalMs: clampInt(options.updateIntervalMs, 100, 10_000, DEFAULTS.updateIntervalMs),
        frameSampleSize: clampInt(options.frameSampleSize, 10, 600, DEFAULTS.frameSampleSize),
        longFrameThresholdMs: clampInt(options.longFrameThresholdMs, 16, 500, DEFAULTS.longFrameThresholdMs),
        maxInvokeStats: clampInt(options.maxInvokeStats, 5, 200, DEFAULTS.maxInvokeStats),
        maxRecentLongFrames: clampInt(options.maxRecentLongFrames, 0, 5000, DEFAULTS.maxRecentLongFrames),
        maxRecentLongTasks: clampInt(options.maxRecentLongTasks, 0, 5000, DEFAULTS.maxRecentLongTasks),
        maxRecentInvokes: clampInt(options.maxRecentInvokes, 0, 5000, DEFAULTS.maxRecentInvokes),
    };

    const state = {
        enabled: false,
        expanded: false,
        installedAt: safeNow(),
        timeOrigin: safeTimeOrigin(),
        pos: readStoredPosition() || { x: 0, y: 0 },
        dragState: null,
        hudEl: null,
        updateTimer: null,
        rafId: null,
        lastFrameTs: null,
        frameDeltas: [],
        recentLongFrames: [],
        longFrameCount: 0,
        longFrameMaxMs: 0,
        longFrameLastMs: 0,
        recentLongTasks: [],
        longTaskCount: 0,
        longTaskMaxMs: 0,
        longTaskLastMs: 0,
        longTaskLastName: null,
        observers: [],
        invokeStatsByCommand: new Map(),
        recentInvokes: [],
        contextRef: null,
        safeInvokeBase: null,
        safeInvokeWrapped: null,
        keyHandlerInstalled: false,
    };

    function isEnabled() {
        return Boolean(state.enabled);
    }

    function recordLongFrame(deltaMs) {
        if (!isEnabled()) {
            return;
        }

        if (deltaMs > config.longFrameThresholdMs) {
            state.longFrameCount += 1;
            state.longFrameLastMs = deltaMs;
            state.longFrameMaxMs = Math.max(state.longFrameMaxMs, deltaMs);

            if (config.maxRecentLongFrames > 0) {
                state.recentLongFrames.push({ ts: safeNow(), deltaMs });
                if (state.recentLongFrames.length > config.maxRecentLongFrames) {
                    state.recentLongFrames.splice(
                        0,
                        state.recentLongFrames.length - config.maxRecentLongFrames,
                    );
                }
            }
        }
    }

    function recordLongTask(entry) {
        if (!isEnabled()) {
            return;
        }

        const duration = Number(entry?.duration);
        if (!Number.isFinite(duration)) {
            return;
        }

        state.longTaskCount += 1;
        state.longTaskLastMs = duration;
        state.longTaskMaxMs = Math.max(state.longTaskMaxMs, duration);
        state.longTaskLastName = typeof entry?.name === 'string' ? entry.name : null;

        if (config.maxRecentLongTasks > 0) {
            const startTime = Number(entry?.startTime);
            state.recentLongTasks.push({
                startTime: Number.isFinite(startTime) ? startTime : null,
                duration,
                name: typeof entry?.name === 'string' ? entry.name : null,
            });
            if (state.recentLongTasks.length > config.maxRecentLongTasks) {
                state.recentLongTasks.splice(
                    0,
                    state.recentLongTasks.length - config.maxRecentLongTasks,
                );
            }
        }
    }

    function recordInvoke(command, durationMs, ok) {
        if (!isEnabled()) {
            return;
        }

        const key = String(command || '').trim() || '(unknown)';
        const existing = state.invokeStatsByCommand.get(key) || {
            count: 0,
            okCount: 0,
            errCount: 0,
            totalMs: 0,
            maxMs: 0,
            lastMs: 0,
            lastAt: 0,
        };

        existing.count += 1;
        if (ok) {
            existing.okCount += 1;
        } else {
            existing.errCount += 1;
        }

        const d = Number(durationMs);
        existing.totalMs += Number.isFinite(d) ? d : 0;
        existing.maxMs = Math.max(existing.maxMs, Number.isFinite(d) ? d : 0);
        existing.lastMs = Number.isFinite(d) ? d : 0;
        existing.lastAt = safeNow();

        state.invokeStatsByCommand.set(key, existing);

        if (config.maxRecentInvokes > 0) {
            state.recentInvokes.push({
                ts: existing.lastAt,
                command: key,
                durationMs: existing.lastMs,
                ok: Boolean(ok),
            });
            if (state.recentInvokes.length > config.maxRecentInvokes) {
                state.recentInvokes.splice(0, state.recentInvokes.length - config.maxRecentInvokes);
            }
        }

        if (state.invokeStatsByCommand.size > config.maxInvokeStats) {
            const oldestKey = [...state.invokeStatsByCommand.entries()]
                .sort((a, b) => (a[1].lastAt || 0) - (b[1].lastAt || 0))[0]?.[0];
            if (oldestKey) {
                state.invokeStatsByCommand.delete(oldestKey);
            }
        }
    }

    function startFrameLoop() {
        state.lastFrameTs = null;
        state.frameDeltas = [];

        const step = (ts) => {
            if (!state.enabled) {
                return;
            }

            if (typeof ts === 'number' && Number.isFinite(ts)) {
                if (state.lastFrameTs !== null) {
                    const delta = ts - state.lastFrameTs;
                    state.frameDeltas.push(delta);
                    if (state.frameDeltas.length > config.frameSampleSize) {
                        state.frameDeltas.splice(0, state.frameDeltas.length - config.frameSampleSize);
                    }
                    recordLongFrame(delta);
                }
                state.lastFrameTs = ts;
            }

            state.rafId = globalThis.requestAnimationFrame(step);
        };

        state.rafId = globalThis.requestAnimationFrame(step);
    }

    function stopFrameLoop() {
        if (state.rafId) {
            try {
                globalThis.cancelAnimationFrame(state.rafId);
            } catch {
                // Ignore.
            }
            state.rafId = null;
        }
    }

    function installObservers() {
        if (!globalThis.PerformanceObserver) {
            return;
        }

        const safeObserve = (observer, init) => {
            try {
                observer.observe(init);
                state.observers.push(observer);
            } catch {
                // Ignore unsupported observer types.
            }
        };

        try {
            const longTaskObserver = new PerformanceObserver((list) => {
                for (const entry of list.getEntries()) {
                    recordLongTask(entry);
                }
            });
            safeObserve(longTaskObserver, { type: 'longtask', buffered: true });
        } catch {
            // Ignore unsupported longtask.
        }

        try {
            const paintObserver = new PerformanceObserver(() => {
                // No-op: we only rely on buffered entries being queryable for HUD rendering.
            });
            safeObserve(paintObserver, { type: 'paint', buffered: true });
        } catch {
            // Ignore unsupported paint entries.
        }

        try {
            const lcpObserver = new PerformanceObserver(() => {
                // No-op: buffered entries queried on render.
            });
            safeObserve(lcpObserver, { type: 'largest-contentful-paint', buffered: true });
        } catch {
            // Ignore unsupported LCP.
        }
    }

    function disconnectObservers() {
        for (const observer of state.observers) {
            try {
                observer.disconnect();
            } catch {
                // Ignore.
            }
        }
        state.observers = [];
    }

    function computeFps() {
        const deltas = state.frameDeltas;
        if (!Array.isArray(deltas) || deltas.length === 0) {
            return null;
        }

        const sum = deltas.reduce((acc, v) => acc + v, 0);
        if (!sum) {
            return null;
        }

        const avgDelta = sum / deltas.length;
        const fps = 1000 / avgDelta;
        return Number.isFinite(fps) ? fps : null;
    }

    function getDomSample() {
        const sample = {
            elements: null,
            mes: null,
            iframes: null,
        };

        try {
            sample.elements = document.getElementsByTagName('*').length;
        } catch {
            // Ignore.
        }

        try {
            sample.mes = document.querySelectorAll('#chat .mes').length;
        } catch {
            // Ignore.
        }

        try {
            sample.iframes = document.querySelectorAll('iframe').length;
        } catch {
            // Ignore.
        }

        return sample;
    }

    function getHeapSample() {
        try {
            const mem = globalThis.performance?.memory;
            if (!mem) {
                return null;
            }
            const used = Number(mem.usedJSHeapSize);
            const total = Number(mem.totalJSHeapSize);
            return Number.isFinite(used) && Number.isFinite(total)
                ? { used, total }
                : null;
        } catch {
            return null;
        }
    }

    function getPaintMetric(entryName) {
        try {
            const entries = globalThis.performance?.getEntriesByName?.(entryName) || [];
            const last = entries[entries.length - 1];
            const start = Number(last?.startTime);
            return Number.isFinite(start) ? start : null;
        } catch {
            return null;
        }
    }

    function getLatestLcp() {
        try {
            const entries = globalThis.performance?.getEntriesByType?.('largest-contentful-paint') || [];
            const last = entries[entries.length - 1];
            const start = Number(last?.startTime);
            return Number.isFinite(start) ? start : null;
        } catch {
            return null;
        }
    }

    function readMeasureDuration(name) {
        try {
            const entries = globalThis.performance?.getEntriesByName?.(name, 'measure') || [];
            const last = entries[entries.length - 1];
            const duration = Number(last?.duration);
            return Number.isFinite(duration) ? duration : null;
        } catch {
            return null;
        }
    }

    function getTopInvokes(limit = 5) {
        const items = [...state.invokeStatsByCommand.entries()]
            .map(([command, stats]) => ({ command, ...stats }))
            .sort((a, b) => (b.totalMs || 0) - (a.totalMs || 0));
        return items.slice(0, Math.max(0, limit));
    }

    function clampHudPosition(x, y) {
        const vw = Number(globalThis.innerWidth) || 0;
        const vh = Number(globalThis.innerHeight) || 0;

        const rect = state.hudEl?.getBoundingClientRect?.();
        const width = Number(rect?.width) || 260;
        const height = Number(rect?.height) || 90;

        const maxX = Math.max(0, vw - width);
        const maxY = Math.max(0, vh - height);

        const nextX = Math.min(Math.max(0, x), maxX);
        const nextY = Math.min(Math.max(0, y), maxY);

        return { x: nextX, y: nextY };
    }

    function applyHudPosition(el) {
        if (!(el instanceof HTMLElement)) {
            return;
        }

        el.style.left = `${Math.round(state.pos.x)}px`;
        el.style.top = `${Math.round(state.pos.y)}px`;
    }

    function setHudPosition(x, y, { persist = false } = {}) {
        const next = clampHudPosition(Number(x) || 0, Number(y) || 0);
        state.pos = next;

        if (state.hudEl) {
            applyHudPosition(state.hudEl);
        }

        if (persist) {
            persistPosition(next);
        }
    }

    function resetHudPosition() {
        setHudPosition(0, 0, { persist: true });
    }

    function ensureHudElement() {
        if (state.hudEl && document.body?.contains?.(state.hudEl)) {
            return state.hudEl;
        }

        const el = document.createElement('div');
        el.id = 'tauritavern-perf-hud';
        el.style.position = 'fixed';
        el.style.top = '0';
        el.style.left = '0';
        el.style.zIndex = '2147483647';
        el.style.padding = '6px 8px';
        el.style.borderRadius = '0 0 10px 0';
        el.style.background = 'rgba(0,0,0,0.72)';
        el.style.color = '#A6FFB5';
        el.style.fontFamily = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", \"Courier New\", monospace';
        el.style.fontSize = '11px';
        el.style.lineHeight = '1.25';
        el.style.whiteSpace = 'pre';
        el.style.maxWidth = 'min(92vw, 520px)';
        el.style.pointerEvents = 'auto';
        el.style.userSelect = 'none';

        const header = document.createElement('div');
        header.textContent = 'Perf HUD (drag to move, tap to expand)';
        header.style.color = '#9EC5FF';
        header.style.marginBottom = '4px';
        header.style.cursor = 'move';
        header.style.touchAction = 'none';
        el.appendChild(header);

        const DRAG_THRESHOLD_PX = 4;

        const getPoint = (ev) => {
            if (ev && typeof ev.clientX === 'number' && typeof ev.clientY === 'number') {
                return { x: ev.clientX, y: ev.clientY };
            }

            const touch = ev?.touches?.[0] || ev?.changedTouches?.[0] || null;
            if (touch && typeof touch.clientX === 'number' && typeof touch.clientY === 'number') {
                return { x: touch.clientX, y: touch.clientY };
            }

            return null;
        };

        const beginDrag = (point, pointerId = null) => {
            if (!point) {
                return;
            }

            state.dragState = {
                pointerId,
                startX: point.x,
                startY: point.y,
                startPos: { ...state.pos },
                moved: false,
            };
        };

        const updateDrag = (point) => {
            const drag = state.dragState;
            if (!drag || !point) {
                return;
            }

            const dx = point.x - drag.startX;
            const dy = point.y - drag.startY;

            if (!drag.moved && (Math.abs(dx) > DRAG_THRESHOLD_PX || Math.abs(dy) > DRAG_THRESHOLD_PX)) {
                drag.moved = true;
            }

            if (!drag.moved) {
                return;
            }

            setHudPosition(drag.startPos.x + dx, drag.startPos.y + dy, { persist: false });
        };

        const endDrag = () => {
            const drag = state.dragState;
            if (!drag) {
                return;
            }

            state.dragState = null;

            if (drag.moved) {
                persistPosition(state.pos);
                return;
            }

            state.expanded = !state.expanded;
            renderHud();
        };

        if (globalThis.PointerEvent) {
            header.addEventListener('pointerdown', (ev) => {
                const point = getPoint(ev);
                beginDrag(point, ev.pointerId);

                try {
                    header.setPointerCapture(ev.pointerId);
                } catch {
                    // Ignore capture failures.
                }

                ev.preventDefault?.();
            });

            header.addEventListener('pointermove', (ev) => {
                updateDrag(getPoint(ev));
            });

            header.addEventListener('pointerup', () => endDrag());
            header.addEventListener('pointercancel', () => endDrag());
        } else {
            header.addEventListener('mousedown', (ev) => {
                beginDrag(getPoint(ev), null);
                ev.preventDefault?.();

                const onMove = (moveEv) => updateDrag(getPoint(moveEv));
                const onUp = () => {
                    window.removeEventListener('mousemove', onMove);
                    window.removeEventListener('mouseup', onUp);
                    endDrag();
                };

                window.addEventListener('mousemove', onMove);
                window.addEventListener('mouseup', onUp);
            });

            header.addEventListener('touchstart', (ev) => {
                beginDrag(getPoint(ev), null);
                ev.preventDefault?.();

                const onMove = (moveEv) => updateDrag(getPoint(moveEv));
                const onEnd = () => {
                    window.removeEventListener('touchmove', onMove);
                    window.removeEventListener('touchend', onEnd);
                    window.removeEventListener('touchcancel', onEnd);
                    endDrag();
                };

                window.addEventListener('touchmove', onMove, { passive: false });
                window.addEventListener('touchend', onEnd);
                window.addEventListener('touchcancel', onEnd);
            }, { passive: false });
        }

        const body = document.createElement('div');
        body.dataset.role = 'body';
        el.appendChild(body);

        document.body.appendChild(el);
        state.hudEl = el;
        setHudPosition(state.pos.x, state.pos.y, { persist: false });
        return el;
    }

    function renderHud() {
        if (!state.enabled) {
            return;
        }

        const root = ensureHudElement();
        const body = root.querySelector('[data-role="body"]');
        if (!(body instanceof HTMLElement)) {
            return;
        }

        const fps = computeFps();
        const dom = getDomSample();
        const heap = getHeapSample();

        const fcp = getPaintMetric('first-contentful-paint');
        const lcp = getLatestLcp();

        const initTotal = readMeasureDuration('tt:init:total');
        const importLib = readMeasureDuration('tt:init:import:lib');
        const importTauriMain = readMeasureDuration('tt:init:import:tauri-main');
        const importApp = readMeasureDuration('tt:init:import:app');

        const lines = [];
        lines.push(`Enabled: yes  Uptime: ${formatMs(safeNow() - state.installedAt)}`);
        lines.push(`FPS: ${fps ? fps.toFixed(1) : '-'}  LongFrames: ${state.longFrameCount} (max ${formatMs(state.longFrameMaxMs)})`);
        lines.push(`LongTasks: ${state.longTaskCount} (max ${formatMs(state.longTaskMaxMs)})`);
        lines.push(`DOM: ${dom.elements ?? '-'}  mes: ${dom.mes ?? '-'}  iframes: ${dom.iframes ?? '-'}`);
        if (heap) {
            lines.push(`Heap: ${formatMB(heap.used)} / ${formatMB(heap.total)}`);
        } else {
            lines.push('Heap: -');
        }

        if (state.expanded) {
            lines.push('');
            lines.push(`FCP: ${fcp === null ? '-' : formatMs(fcp)}  LCP: ${lcp === null ? '-' : formatMs(lcp)}`);
            lines.push(`Init total: ${initTotal === null ? '-' : formatMs(initTotal)}`);
            lines.push(`Import lib: ${importLib === null ? '-' : formatMs(importLib)}`);
            lines.push(`Import tauri-main: ${importTauriMain === null ? '-' : formatMs(importTauriMain)}`);
            lines.push(`Import app: ${importApp === null ? '-' : formatMs(importApp)}`);

            const top = getTopInvokes(6);
            if (top.length) {
                lines.push('');
                lines.push('Top invokes (total time):');
                for (const item of top) {
                    const errHint = item.errCount ? ` err:${item.errCount}` : '';
                    lines.push(`- ${item.command}: ${formatMs(item.totalMs)} (n:${item.count} max:${formatMs(item.maxMs)}${errHint})`);
                }
            }

            lines.push('');
            lines.push(`timeOrigin: ${Math.round(state.timeOrigin)}  now(): ${Math.round(safeNow())}`);
            lines.push('Commands: __TAURITAVERN_PERF__.exportJson() / downloadReport() / copyReport() / saveReport()');
            lines.push('Auto-enable: localStorage tt:perf=1');
        }

        body.textContent = lines.join('\n');
    }

    function startHudLoop() {
        if (state.updateTimer) {
            return;
        }

        state.updateTimer = globalThis.setInterval(() => {
            renderHud();
        }, config.updateIntervalMs);
    }

    function stopHudLoop() {
        if (state.updateTimer) {
            try {
                globalThis.clearInterval(state.updateTimer);
            } catch {
                // Ignore.
            }
            state.updateTimer = null;
        }
    }

    function ensureKeyHandler() {
        if (state.keyHandlerInstalled) {
            return;
        }
        state.keyHandlerInstalled = true;

        window.addEventListener('keydown', (ev) => {
            if (!ev || typeof ev.key !== 'string') {
                return;
            }
            // Ctrl+Alt+P toggles the HUD (dev convenience).
            if (ev.key.toLowerCase() !== 'p') {
                return;
            }
            if (!ev.ctrlKey || !ev.altKey) {
                return;
            }
            ev.preventDefault?.();
            toggle();
        });
    }

    function attachContext(context) {
        if (!context || typeof context.safeInvoke !== 'function') {
            return;
        }

        if (state.contextRef && state.contextRef !== context) {
            restoreSafeInvokeWrapper(state.contextRef);
        }

        state.contextRef = context;

        if (state.enabled) {
            ensureSafeInvokeWrapperInstalled(context);
        }
    }

    function ensureSafeInvokeWrapperInstalled(context) {
        const target = context || state.contextRef;
        if (!target || typeof target.safeInvoke !== 'function') {
            return;
        }

        if (target.safeInvoke === state.safeInvokeWrapped) {
            return;
        }

        const current = target.safeInvoke;
        const base = (current && current.__ttPerfWrapped && typeof current.__ttPerfBase === 'function')
            ? current.__ttPerfBase
            : current;

        if (typeof base !== 'function') {
            return;
        }

        const wrapped = async (command, args = {}) => {
            const t0 = safeNow();
            try {
                const result = await base(command, args);
                recordInvoke(command, safeNow() - t0, true);
                return result;
            } catch (error) {
                recordInvoke(command, safeNow() - t0, false);
                throw error;
            }
        };

        wrapped.__ttPerfWrapped = true;
        wrapped.__ttPerfBase = base;

        target.safeInvoke = wrapped;
        state.safeInvokeBase = base;
        state.safeInvokeWrapped = wrapped;
    }

    function restoreSafeInvokeWrapper(context) {
        const target = context || state.contextRef;
        if (!target) {
            state.safeInvokeBase = null;
            state.safeInvokeWrapped = null;
            return;
        }

        if (state.safeInvokeWrapped && target.safeInvoke === state.safeInvokeWrapped) {
            const base = state.safeInvokeBase;
            if (typeof base === 'function') {
                target.safeInvoke = base;
            }
        }

        state.safeInvokeBase = null;
        state.safeInvokeWrapped = null;
    }

    function enable() {
        if (state.enabled) {
            return;
        }

        state.enabled = true;
        persistEnabledFlag(true);
        safeMark('tt:perf:enabled');

        ensureSafeInvokeWrapperInstalled();
        ensureKeyHandler();
        installObservers();
        startFrameLoop();
        startHudLoop();
        renderHud();
    }

    function disable() {
        if (!state.enabled) {
            return;
        }

        state.enabled = false;
        persistEnabledFlag(false);
        safeMark('tt:perf:disabled');

        stopHudLoop();
        stopFrameLoop();
        disconnectObservers();
        restoreSafeInvokeWrapper();

        if (state.hudEl) {
            try {
                state.hudEl.remove();
            } catch {
                // Ignore.
            }
            state.hudEl = null;
        }
    }

    function toggle() {
        if (state.enabled) {
            disable();
        } else {
            enable();
        }
    }

    function snapshot() {
        const fps = computeFps();
        const dom = getDomSample();
        const heap = getHeapSample();

        return {
            enabled: state.enabled,
            now: safeNow(),
            timeOrigin: state.timeOrigin,
            fps,
            longFrames: {
                count: state.longFrameCount,
                maxMs: state.longFrameMaxMs,
                lastMs: state.longFrameLastMs,
            },
            longTasks: {
                count: state.longTaskCount,
                maxMs: state.longTaskMaxMs,
                lastMs: state.longTaskLastMs,
                lastName: state.longTaskLastName,
            },
            dom,
            heap,
            init: {
                totalMs: readMeasureDuration('tt:init:total'),
                importLibMs: readMeasureDuration('tt:init:import:lib'),
                importTauriMainMs: readMeasureDuration('tt:init:import:tauri-main'),
                importAppMs: readMeasureDuration('tt:init:import:app'),
            },
            topInvokes: getTopInvokes(10),
        };
    }

    function serializePerformanceEntry(entry) {
        if (!entry || typeof entry !== 'object') {
            return null;
        }

        const startTime = Number(entry.startTime);
        const duration = Number(entry.duration);

        const serialized = {
            name: typeof entry.name === 'string' ? entry.name : '',
            entryType: typeof entry.entryType === 'string' ? entry.entryType : '',
            startTime: Number.isFinite(startTime) ? startTime : null,
            duration: Number.isFinite(duration) ? duration : null,
        };

        for (const extraKey of ['initiatorType', 'renderTime', 'loadTime', 'size', 'url']) {
            const v = entry[extraKey];
            if (typeof v === 'string' || Number.isFinite(Number(v))) {
                serialized[extraKey] = v;
            }
        }

        return serialized;
    }

    function collectPerformanceEntries({ maxEntries = 2000, includeResources = false } = {}) {
        const max = clampInt(maxEntries, 0, 50_000, 2000);

        const collect = (type, predicate) => {
            try {
                const entries = globalThis.performance?.getEntriesByType?.(type) || [];
                const filtered = predicate ? entries.filter(predicate) : entries;
                const sliced = max ? filtered.slice(-max) : filtered;
                return sliced.map(serializePerformanceEntry).filter(Boolean);
            } catch {
                return [];
            }
        };

        const nameStartsWithTt = (entry) => typeof entry?.name === 'string' && entry.name.startsWith('tt:');

        const payload = {
            marks: collect('mark', nameStartsWithTt),
            measures: collect('measure', nameStartsWithTt),
            paints: collect('paint'),
            lcp: collect('largest-contentful-paint'),
            navigation: collect('navigation'),
        };

        if (includeResources) {
            payload.resources = collect('resource');
        }

        return payload;
    }

    function report(options = {}) {
        const snapshotData = snapshot();

        const env = {
            createdAt: new Date().toISOString(),
            location: {
                origin: String(globalThis.location?.origin || ''),
                href: String(globalThis.location?.href || ''),
            },
            userAgent: typeof navigator?.userAgent === 'string' ? navigator.userAgent : null,
            platform: typeof navigator?.platform === 'string' ? navigator.platform : null,
            languages: Array.isArray(navigator?.languages) ? navigator.languages : null,
            hardwareConcurrency: Number.isFinite(Number(navigator?.hardwareConcurrency)) ? navigator.hardwareConcurrency : null,
            deviceMemory: Number.isFinite(Number(navigator?.deviceMemory)) ? navigator.deviceMemory : null,
            maxTouchPoints: Number.isFinite(Number(navigator?.maxTouchPoints)) ? navigator.maxTouchPoints : null,
            viewport: {
                innerWidth: Number.isFinite(Number(globalThis.innerWidth)) ? globalThis.innerWidth : null,
                innerHeight: Number.isFinite(Number(globalThis.innerHeight)) ? globalThis.innerHeight : null,
                devicePixelRatio: Number.isFinite(Number(globalThis.devicePixelRatio)) ? globalThis.devicePixelRatio : null,
            },
            screen: globalThis.screen
                ? {
                    width: screen.width,
                    height: screen.height,
                    availWidth: screen.availWidth,
                    availHeight: screen.availHeight,
                }
                : null,
        };

        const invokeStats = Object.fromEntries(state.invokeStatsByCommand.entries());

        const perfEntries = options.includePerfEntries === false
            ? null
            : collectPerformanceEntries({
                maxEntries: options.maxPerfEntries ?? 2000,
                includeResources: Boolean(options.includeResources),
            });

        return {
            kind: 'tauritavern-perf-report',
            version: 1,
            config,
            hud: {
                enabled: state.enabled,
                expanded: state.expanded,
                position: state.pos,
            },
            env,
            snapshot: snapshotData,
            invokes: {
                statsByCommand: invokeStats,
                recent: state.recentInvokes.slice(),
            },
            longTasks: {
                recent: state.recentLongTasks.slice(),
            },
            longFrames: {
                recent: state.recentLongFrames.slice(),
            },
            perfEntries,
        };
    }

    function exportJson(options = {}) {
        const pretty = options.pretty !== false;
        const payload = report(options);

        return JSON.stringify(payload, null, pretty ? 2 : 0);
    }

    function printReport(options = {}) {
        const payload = report(options);
        console.log('[TauriTavern Perf Report]', payload);
        return payload;
    }

    function printJson(options = {}) {
        const json = exportJson(options);
        console.log(json);
        return json;
    }

    async function copyReport(options = {}) {
        const json = exportJson(options);
        const clipboard = navigator?.clipboard;
        if (!clipboard || typeof clipboard.writeText !== 'function') {
            return false;
        }

        try {
            await clipboard.writeText(json);
            return true;
        } catch {
            return false;
        }
    }

    function downloadReport(options = {}) {
        const json = exportJson(options);
        const suffix = new Date().toISOString().replace(/[:.]/g, '-');
        const filename = String(options.filename || `tauritavern-perf-${suffix}.json`);

        const blob = new Blob([json], { type: 'application/json' });
        const url = URL.createObjectURL(blob);

        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = filename;
        document.body.appendChild(anchor);
        anchor.click();
        anchor.remove();

        setTimeout(() => URL.revokeObjectURL(url), 0);
        return { filename, bytes: json.length };
    }

    function saveReport(options = {}) {
        const json = exportJson(options);
        const key = String(options.storageKey || 'tt:perf:lastReport');
        try {
            globalThis.localStorage?.setItem(key, json);
            return { key, bytes: json.length };
        } catch (error) {
            return { key, bytes: json.length, error: String(error || 'save failed') };
        }
    }

    return {
        config,
        get enabled() {
            return state.enabled;
        },
        shouldAutoEnable: readAutoEnableFlag,
        enable,
        disable,
        toggle,
        snapshot,
        report,
        exportJson,
        printReport,
        printJson,
        copyReport,
        downloadReport,
        saveReport,
        attachContext,
        setPosition: (x, y) => setHudPosition(x, y, { persist: true }),
        resetPosition: resetHudPosition,
        mark: safeMark,
        measure: safeMeasure,
    };
}

export function installPerfHud({ context, options } = {}) {
    try {
        const existing = globalThis[GLOBAL_KEY];
        if (existing && typeof existing === 'object' && typeof existing.enable === 'function') {
            if (context) {
                existing.attachContext?.(context);
            }
            return existing;
        }
    } catch {
        // Continue with a fresh controller.
    }

    const controller = createPerfController(options);
    try {
        Object.defineProperty(globalThis, GLOBAL_KEY, {
            value: controller,
            configurable: true,
            writable: false,
            enumerable: false,
        });
    } catch {
        globalThis[GLOBAL_KEY] = controller;
    }

    if (context) {
        controller.attachContext(context);
    }

    if (controller.shouldAutoEnable()) {
        controller.enable();
    }

    safeMark('tt:perf:installed');
    return controller;
}
