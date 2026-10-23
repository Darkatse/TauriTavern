// Runs the built frontend in Playwright's pinned headless Chromium, embedded through the Web Host, and
// counts the engine work each scenario step causes. Counts depend on the code, DOM and CSS, not on the
// machine, so they can be compared against a committed baseline.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { defaultSettings, defaultTheme, frontendRoot, systemExtensions } from '../helpers/frontend-content.mjs';

const fixtureFile = fileURLToPath(new URL('../browser/web-host-fixture.mjs', import.meta.url));
const TYPES = {
    '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json',
    '.png': 'image/png', '.svg': 'image/svg+xml', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf',
};

/** A long chat and a large lorebook; the first entry holds 100k characters. */
function dataset() {
    const paragraph = 'The old lighthouse keeper remembers every ship that passed the northern reef, and speaks of them as friends. ';
    const chat = [{ user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } }];
    for (let i = 0; i < 80; i++) {
        chat.push({
            name: i % 2 ? 'User' : 'Review', is_user: i % 2 === 1, is_system: false, send_date: '2026-01-01T12:00:00Z',
            // Replies carry their model icon, as in a real chat, so a new reply's icon is already loaded.
            extra: i % 2 ? {} : { api: 'custom', model: 'replay' },
            mes: `*She glances at the map.* "${paragraph.repeat(3)}"\n\n**Note:** ${paragraph}`,
        });
    }
    const entries = {};
    for (let uid = 0; uid < 200; uid++) {
        entries[uid] = {
            uid, key: [`key${uid}`, `alias${uid}`], keysecondary: [], comment: `Entry ${uid}`, content: paragraph.repeat(8),
            constant: false, selective: true, order: 100, position: 0, disable: false, displayIndex: uid, probability: 100, useProbability: true,
        };
    }
    entries[0].content = `${paragraph.repeat(4)}\n\n`.repeat(250).slice(0, 100_000);
    return { chat, lore: { entries } };
}

function bootstrap() {
    const settings = structuredClone(defaultSettings);
    // With the browser flags in openFrontend, no animation spreads a step's work over time.
    settings.power_user.reduced_motion = true;
    const data = { settings, theme: defaultTheme, extensions: systemExtensions, ...dataset() };
    return `<script>
// Timer timing must not decide how DOM changes batch into frames:
// - a timeout shorter than a frame runs just before the next frame, so a chain of them lands in one frame;
// - during a step, a longer one, up to the relaxed debounce (1 s), waits until the page is quiet, when the
//   harness runs every pending timeout in due order. Outside steps it keeps real time, so scenarios prepare
//   the page as a user would.
const pending = new Map();
const { setTimeout: schedule, clearTimeout: cancel, requestAnimationFrame: nextFrame } = globalThis;
let heldId = 0, frameRequested = false;
function runPending(filter) {
    let runs = 0;
    for (let first; ; first = undefined) {
        for (const entry of pending) if (filter(entry[1]) && (!first || entry[1].due < first[1].due)) first = entry;
        if (!first) return runs;
        if (++runs > 1000) throw new Error('Timeouts keep scheduling each other');
        cancel(first[0]);
        first[1].run();
    }
}
function requestFrame() {
    if (frameRequested) return;
    frameRequested = true;
    nextFrame(() => {
        frameRequested = false;
        runPending(timeout => timeout.short);
    });
}
globalThis.setTimeout = (callback, delay = 0, ...args) => {
    if (delay > 1000) return schedule(callback, delay, ...args);
    const timeout = {
        due: performance.now() + delay,
        short: delay < 16,
        run: () => { pending.delete(id); return typeof callback === 'function' ? callback(...args) : globalThis.eval(callback); },
    };
    // Held timeouts take ids below zero, which no real timer uses.
    const id = timeout.short || globalThis.__perfHolding ? --heldId : schedule(timeout.run, delay);
    pending.set(id, timeout);
    if (timeout.short) requestFrame();
    return id;
};
globalThis.clearTimeout = id => { pending.delete(id); cancel(id); };
globalThis.__perfRunPendingTimeouts = () => runPending(() => true);
</script>
<script type="application/json" id="perf-data">${JSON.stringify(data).replace(/</g, '\\u003c')}</script>
<script type="module">
import { installWebHost } from '/tauri/web-host/install.js';
import { createWebHostFixture } from '/__perf/web-host-fixture.mjs';
const data = JSON.parse(document.getElementById('perf-data').textContent);
const fixture = createWebHostFixture(data.settings, data.theme, data.extensions);
fixture.chatFiles.set('review-chat', data.chat.map(record => JSON.stringify(record)).join('\\n'));
// Opens the reply stream that the scenario feeds through __perfReply.
function streamReply() {
    const encoder = new TextEncoder();
    return new ReadableStream({
        start(controller) {
            const send = data => controller.enqueue(encoder.encode(\`data: \${data}\\n\\n\`));
            globalThis.__perfReply = {
                push: token => send(JSON.stringify({ choices: [{ delta: { content: token }, index: 0 }] })),
                end() {
                    send('[DONE]');
                    controller.close();
                },
            };
        },
    });
}
installWebHost({
    identity: { platform: 'windows', kind: 'desktop' },
    backend: { handle(request) {
        if (request.method === 'POST' && request.url === '/api/worldinfo/get') {
            return { status: 200, headers: { 'content-type': 'application/json' }, body: JSON.stringify(data.lore) };
        }
        if (request.method === 'POST' && request.url === '/api/backends/chat-completions/generate' && JSON.parse(request.body).stream) {
            return { status: 200, headers: { 'content-type': 'text/event-stream' }, body: streamReply() };
        }
        return fixture.backend.handle(request);
    } },
    store: fixture.store,
});
</script>`;
}

// What the native host serves from the user's data directory; this dataset has none of it.
const USER_DATA = /^\/(?:thumbnail$|backgrounds\/|css\/user\.css$)/;
const EMPTY_PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=', 'base64');

async function serve() {
    const index = (await readFile(path.join(frontendRoot, 'index.html'), 'utf8'))
        .replace('<script type="module" src="init.js"></script>', `${bootstrap()}\n<script type="module" src="init.js"></script>`);
    async function resource(pathname) {
        if (pathname === '/') return { type: 'text/html', body: index };
        if (USER_DATA.test(pathname)) return pathname.endsWith('.css') ? { type: 'text/css', body: '' } : { type: 'image/png', body: EMPTY_PNG };
        const file = pathname === '/__perf/web-host-fixture.mjs' ? fixtureFile : path.join(frontendRoot, decodeURIComponent(pathname));
        return { type: TYPES[path.extname(file)] ?? 'application/octet-stream', body: await readFile(file) };
    }
    const server = createServer(async (request, response) => {
        try {
            const { type, body } = await resource(new URL(request.url, 'http://localhost').pathname);
            response.writeHead(200, { 'content-type': type });
            response.end(body);
        } catch {
            response.writeHead(404);
            response.end();
        }
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    return server;
}

/** Engine work inside a trace, taken from the renderer's main thread. */
function countEngineWork(events) {
    const threads = new Map();
    for (const event of events) {
        if (event.ph === 'M' && event.name === 'thread_name' && event.args.name === 'CrRendererMain') threads.set(`${event.pid}:${event.tid}`, 0);
    }
    for (const event of events) {
        const thread = `${event.pid}:${event.tid}`;
        if (threads.has(thread)) threads.set(thread, threads.get(thread) + 1);
    }
    const main = [...threads].sort((a, b) => b[1] - a[1])[0][0];
    const work = events.filter(event => event.ph === 'X' && `${event.pid}:${event.tid}` === main).sort((a, b) => a.ts - b.ts);
    const counters = {
        eventDispatches: 0, styleRecalcs: 0, forcedStyleRecalcs: 0, elementsRestyled: 0,
        layouts: 0, forcedLayouts: 0, layoutObjectsLaidOut: 0, frames: 0, compositorUpdates: 0,
    };
    // A frame counts when it renders style or layout work; idle repaints such as a blinking caret do not.
    let rendering = false, counted = false;
    for (const event of work) {
        const forced = Boolean(event.args?.beginData?.stackTrace);
        switch (event.name) {
            case 'EventDispatch':
                // The engine coalesces slotchange per microtask checkpoint, so its count follows scheduling.
                counters.eventDispatches += event.args?.data?.type !== 'slotchange';
                break;
            case 'UpdateLayoutTree':
                counters.styleRecalcs++;
                counters.forcedStyleRecalcs += forced;
                counters.elementsRestyled += event.args?.elementCount ?? 0;
                rendering = true;
                break;
            case 'Layout':
                counters.layouts++;
                counters.forcedLayouts += forced;
                counters.layoutObjectsLaidOut += event.args?.beginData?.dirtyObjects ?? 0;
                rendering = true;
                break;
            case 'PrePaint':
                counted = rendering;
                rendering = false;
                counters.frames += counted;
                break;
            case 'Layerize':
                counters.compositorUpdates += counted;
                break;
        }
    }
    return counters;
}

/**
 * Opens the frontend and returns the scenario API. `measure(name, ...inputs)` runs the inputs in order and
 * settles the page after each one, so the machine's speed never decides which work shares a frame. It
 * records the engine work they caused plus the page's size afterwards.
 */
export async function openFrontend() {
    const server = await serve();
    // Animations, including smooth scrolling, would make frame counts follow elapsed time instead of work.
    const browser = await chromium.launch({ args: ['--disable-smooth-scrolling', '--force-prefers-reduced-motion'] });
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Performance.enable');
    const problems = [];
    page.on('pageerror', error => problems.push(`Page error: ${error.message}`));
    page.on('console', message => { if (message.type() === 'error') problems.push(`Console error: ${message.text()}`); });
    page.on('response', response => { if (response.status() >= 400) problems.push(`HTTP ${response.status()} ${new URL(response.url()).pathname}`); });
    const results = {};
    let requests = 0;
    page.on('request', () => requests++);
    // During a step, network responses are held like longer timeouts: settle() lets them through one at a time.
    let holding = false;
    const held = [];
    await page.route('**/*', route => holding ? held.push(route) : route.continue());
    async function hold(active) {
        holding = active;
        await page.evaluate(value => { globalThis.__perfHolding = value; }, active);
    }

    // Waits until the main thread has idled for three frame intervals, then runs the held timeouts or lets one
    // held response through, and repeats until nothing is held. Holding them means no work arrives after the
    // page looks quiet. Idleness is read from outside the page, so waiting adds no page work.
    async function settle() {
        const busy = async () => (await cdp.send('Performance.getMetrics')).metrics.find(metric => metric.name === 'TaskDuration').value;
        const deadline = Date.now() + 5000;
        for (;;) {
            for (let quiet = 0, last = await busy(); quiet < 50;) {
                if (Date.now() > deadline) throw new Error('The page did not go quiet within 5 s');
                await new Promise(resolve => setTimeout(resolve, 10));
                const now = await busy();
                quiet = now - last < 0.001 ? quiet + 10 : 0;
                last = now;
            }
            if (await page.evaluate(() => globalThis.__perfRunPendingTimeouts?.() ?? 0) > 0) continue;
            const route = held.shift();
            if (!route) return;
            await route.continue();
            await (await route.request().response())?.finished();
        }
    }

    async function trace(step) {
        const events = [];
        const collect = ({ value }) => events.push(...value);
        cdp.on('Tracing.dataCollected', collect);
        await cdp.send('Tracing.start', {
            transferMode: 'ReportEvents',
            traceConfig: { includedCategories: ['devtools.timeline', 'disabled-by-default-devtools.timeline', 'disabled-by-default-devtools.timeline.stack'] },
        });
        await step();
        const complete = new Promise(resolve => cdp.once('Tracing.tracingComplete', resolve));
        await cdp.send('Tracing.end');
        await complete;
        cdp.off('Tracing.dataCollected', collect);
        return countEngineWork(events);
    }

    // Measured after a forced layout and a collection, so the layout tree is complete and only retained
    // listeners and nodes count.
    async function pageSize() {
        const dom = await page.evaluate(() => {
            const rendered = [...document.querySelectorAll('*')].filter(element => element.getClientRects().length > 0);
            return {
                elements: document.getElementsByTagName('*').length,
                scrollContainers: rendered.filter(element => {
                    const style = getComputedStyle(element);
                    return /auto|scroll/.test(`${style.overflowX} ${style.overflowY}`);
                }).length,
            };
        });
        await cdp.send('HeapProfiler.collectGarbage');
        const { metrics } = await cdp.send('Performance.getMetrics');
        const metric = name => metrics.find(entry => entry.name === name).value;
        return { ...dom, liveNodes: metric('Nodes'), layoutObjects: metric('LayoutObjects'), eventListeners: metric('JSEventListeners') };
    }

    async function measure(name, ...inputs) {
        if (name in results) throw new Error(`Duplicate perf step: ${name}`);
        await settle();
        const before = requests;
        await hold(true);
        const work = await trace(async () => {
            for (const input of inputs) {
                await input();
                await settle();
            }
        });
        await hold(false);
        results[name] = { ...work, requests: requests - before, ...await pageSize() };
    }

    // Startup's work depends on when resources finish loading, so it records the page it produces.
    try {
        // index.html declares <base href="/">: with a query string, jQuery UI Tabs treats its tabs as remote and
        // loads the whole page into a tab panel.
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        // The fixture's character opens its chat, review-chat, once startup has finished.
        await page.waitForFunction(() => globalThis.__TAURITAVERN_STARTUP_STAGE__ === 'full'
            && globalThis.SillyTavern?.getContext().chatId === 'review-chat', null, { polling: 100 });
        await settle();
        results['app/startup'] = await pageSize();
    } catch (error) {
        await browser.close();
        server.close();
        throw error;
    }

    /** The center of a Playwright locator, resolved before a step so that clicking inside it adds no driver events. */
    async function centerOf(locator) {
        const box = await locator.boundingBox({ timeout: 5000 });
        if (!box) throw new Error(`Not visible: ${locator}`);
        return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    }

    return {
        page,
        measure,
        centerOf,
        results,
        problems,
        engine: `Chromium ${browser.version()}`,
        async close() {
            await browser.close();
            server.close();
        },
    };
}
