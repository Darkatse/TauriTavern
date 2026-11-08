// The frontend layer: runs the built frontend in Playwright's pinned headless Chromium, embedded through the Web
// Host, and counts the engine work, host traffic and JavaScript allocations each scenario step causes, and the
// host calls the backend layer replays.
import { createServer } from 'node:http';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { frontendRoot } from '../helpers/frontend-content.mjs';
import { CONTENT_TYPES, frontendFile } from '../helpers/frontend-server.mjs';
import { pageData } from './dataset.mjs';

const fixtureFile = fileURLToPath(new URL('../helpers/web-host-fixture.mjs', import.meta.url));

function bootstrap() {
    const data = pageData();
    return `<script>
// Timers and clocks must not decide how DOM changes batch into frames, or which step does a piece of work:
// - a timeout shorter than a frame runs just before the next frame, so a chain of them lands in one frame;
// - during a step, a timeout up to 10 s, which covers debounces and a toast's lifetime, waits until the page is
//   quiet, when the harness runs every pending timeout in due order on a clock that only those runs advance; an
//   interval the step starts ticks once each time the page goes quiet. Timers from before the step wait until it
//   ends. performance.now() and Date.now() read that clock too, so work the page slices by elapsed time does
//   not split by machine speed. Outside steps timers and clocks keep real time, so scenarios prepare the page as
//   a user would.
// - frame and idle callbacks the page requests are pending work: the page is not quiet until they have run.
// A step leaves nothing it scheduled behind; the harness fails the run on anything left.
// Block scope keeps these names out of the global scope that classic scripts share.
{
    const pending = new Map(), intervals = new Map(), keptIntervals = new Map(), stepTimeouts = new Map();
    const callbacks = new Set(), afterStep = [];
    const {
        setTimeout: schedule, clearTimeout: cancel, setInterval: repeat, clearInterval: stopRepeating,
        requestAnimationFrame: nextFrame, cancelAnimationFrame: cancelFrame,
        requestIdleCallback: whenIdle, cancelIdleCallback: cancelIdle,
    } = globalThis;
    const realNow = performance.now.bind(performance), realDateNow = Date.now;
    let heldId = 0, frameRequested = false, holding = false, stepClock = 0, stepEpoch = 0, activity = 0;
    const now = () => holding ? stepClock : realNow();
    performance.now = now;
    Date.now = () => holding ? Math.floor(stepEpoch + stepClock) : realDateNow();
    const source = () => new Error().stack.split('\\n').slice(3, 5).map(line => line.trim()).join(' <- ');
    const run = (callback, args) => {
        activity++;
        return typeof callback === 'function' ? callback(...args) : globalThis.eval(callback);
    };
    function runPending(filter) {
        let runs = 0;
        for (let first; ; first = undefined) {
            for (const entry of pending) if (filter(entry[1]) && (!first || entry[1].due < first[1].due)) first = entry;
            if (!first) return runs;
            if (++runs > 1000) throw new Error('Timeouts keep scheduling each other');
            cancel(first[0]);
            if (holding) stepClock = Math.max(stepClock, first[1].due);
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
        if (delay > 10_000) {
            if (!holding) return schedule(() => holding ? afterStep.push(() => run(callback, args)) : run(callback, args), delay);
            const id = schedule(() => { stepTimeouts.delete(id); run(callback, args); }, delay);
            stepTimeouts.set(id, \`timeout of \${delay} ms from \${source()}\`);
            return id;
        }
        const timeout = {
            due: now() + delay,
            short: delay < 16,
            run: () => { pending.delete(id); return run(callback, args); },
        };
        // Held timeouts take ids below zero, which no real timer uses. A real one that comes due during a step waits
        // for the harness, which runs it when the page is quiet.
        const id = timeout.short || holding ? --heldId : schedule(() => holding || timeout.run(), delay);
        pending.set(id, timeout);
        if (timeout.short) requestFrame();
        return id;
    };
    globalThis.setInterval = (callback, delay = 0, ...args) => {
        if (!holding) return repeat(() => holding || run(callback, args), delay);
        const id = --heldId;
        intervals.set(id, { tick: () => run(callback, args), delay, from: source() });
        return id;
    };
    // Timeouts and intervals share one id space, so either function clears both.
    globalThis.clearTimeout = globalThis.clearInterval = id => {
        pending.delete(id);
        stepTimeouts.delete(id);
        intervals.delete(id);
        stopRepeating(keptIntervals.get(id));
        keptIntervals.delete(id);
        cancel(id);
        stopRepeating(id);
    };
    globalThis.requestAnimationFrame = callback => {
        const id = nextFrame(time => { callbacks.delete(\`frame \${id}\`); run(callback, [time]); });
        callbacks.add(\`frame \${id}\`);
        return id;
    };
    globalThis.cancelAnimationFrame = id => { callbacks.delete(\`frame \${id}\`); cancelFrame(id); };
    globalThis.requestIdleCallback = (callback, options) => {
        const id = whenIdle(deadline => { callbacks.delete(\`idle \${id}\`); run(callback, [deadline]); }, options);
        callbacks.add(\`idle \${id}\`);
        return id;
    };
    globalThis.cancelIdleCallback = id => { callbacks.delete(\`idle \${id}\`); cancelIdle(id); };
    globalThis.__perfPage = {
        startStep() {
            stepClock = realNow();
            stepEpoch = realDateNow() - stepClock;
            holding = true;
        },
        /** Ends the step and returns what it left scheduled. Intervals it left keep running in real time. */
        endStep() {
            holding = false;
            const left = [...stepTimeouts.values()];
            for (const [id, interval] of intervals) {
                left.push(\`interval of \${interval.delay} ms from \${interval.from}\`);
                keptIntervals.set(id, repeat(() => holding || interval.tick(), interval.delay));
            }
            intervals.clear();
            stepTimeouts.clear();
            for (const deferred of afterStep.splice(0)) schedule(deferred);
            return left;
        },
        runPendingTimeouts: () => runPending(() => true),
        /** Ticks each interval the step started once; true when a tick cleared one or scheduled a timeout. */
        tickIntervals() {
            const before = \`\${intervals.size}:\${pending.size}\`;
            for (const [id, interval] of [...intervals]) if (intervals.has(id)) interval.tick();
            return \`\${intervals.size}:\${pending.size}\` !== before;
        },
        hasPendingCallbacks: () => callbacks.size > 0,
        /** Counts timer, interval and callback runs, and the host calls the host wrapper reports. */
        activity: () => activity,
        noteActivity() { activity++; },
    };
}
</script>
<script type="application/json" id="perf-data">${JSON.stringify(data).replace(/</g, '\\u003c')}</script>
<script type="module">
import { installWebHost } from '/tauri/web-host/install.js';
import { createWebHostFixture } from '/__perf/web-host-fixture.mjs';
const data = JSON.parse(document.getElementById('perf-data').textContent);
const fixture = createWebHostFixture(data.settings, data.theme, data.extensions);
fixture.chatFiles.set('review-chat', data.chat.map(record => JSON.stringify(record)).join('\\n'));
fixture.chatFiles.set('ledger-chat', data.ledgerChat.map(record => JSON.stringify(record)).join('\\n'));
fixture.chatFiles.set('atlas-chat', data.atlasChat.map(record => JSON.stringify(record)).join('\\n'));
const respond = (body, status = 200) => ({ status, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
const cards = {
    'Review.png': () => JSON.parse(fixture.backend.handle({ method: 'POST', url: '/api/characters/get', headers: {}, body: '{}' }).body),
    'Ledger.png': () => ({ ...JSON.parse(data.ledgerCard), json_data: data.ledgerCard, avatar: 'Ledger.png',
        chat_size: 160000, data_size: 2100000, date_added: 2, date_last_chat: 2 }),
    'Atlas.png': () => ({ ...JSON.parse(data.atlasCard), json_data: data.atlasCard, avatar: 'Atlas.png',
        chat_size: 790000, data_size: 95000, date_added: 3, date_last_chat: 3 }),
};
const chatsOf = { 'Review.png': ['review-chat'], 'Ledger.png': ['ledger-chat'], 'Atlas.png': ['atlas-chat'] };
// The native host's list entry (tt-domain Character::into_shallow); the full card is read when one opens.
function shallow(card) {
    const data = card.data ?? {}, extensions = data.extensions ?? {};
    const pick = (value, fallback) => String(value ?? '').trim() ? value : fallback ?? '';
    const talkativeness = card.talkativeness || extensions.talkativeness || 0.5;
    const fav = Boolean(card.fav) || Boolean(extensions.fav);
    return {
        shallow: true, name: pick(card.name, data.name), description: '', personality: '', scenario: '', first_mes: '', mes_example: '',
        avatar: card.avatar, chat: card.chat, creator: pick(card.creator, data.creator), creator_notes: pick(card.creator_notes, data.creator_notes),
        character_version: pick(card.character_version, data.character_version), tags: card.tags?.length ? card.tags : data.tags ?? [],
        create_date: card.create_date, talkativeness, fav, chat_size: card.chat_size, data_size: card.data_size,
        date_added: card.date_added, date_last_chat: card.date_last_chat, alternate_greetings: [], system_prompt: '',
        post_history_instructions: '', character_book: null, json_data: null,
        data: { extensions: { talkativeness, fav, ...('world' in extensions ? { world: extensions.world } : {}) } },
    };
}
function handleCharacters(request) {
    const body = typeof request.body === 'string' && request.body ? JSON.parse(request.body) : {};
    switch (\`\${request.method} \${request.url}\`) {
        case 'POST /api/characters/all': return respond(Object.values(cards).map(card => shallow(card())));
        case 'POST /api/characters/get':
            return cards[body.avatar_url] ? respond(cards[body.avatar_url]()) : respond({ error: 'Character not found' }, 404);
        case 'POST /api/characters/chats': {
            const owned = chatsOf[body.avatar_url] ?? [];
            const chats = JSON.parse(fixture.backend.handle(request).body);
            return respond(chats.filter(chat => owned.includes(chat.file_name.replace(/\\.jsonl$/, ''))));
        }
        case 'POST /api/characters/edit':
        case 'POST /api/characters/merge-attributes': {
            // The fixture stores one card, Review.
            const fields = request.body?.kind === 'form' ? Object.fromEntries(request.body.entries) : body;
            return (fields.avatar_url ?? fields.avatar) === 'Review.png' ? null : respond({ error: 'Only Review is writable' }, 500);
        }
        default: return null;
    }
}
// Opens the reply stream that the scenario feeds through __perfReply while it is open. A token arrives 50 ms after
// it is pushed; during a step that time passes on the step clock, so every token passes the 30 FPS render limit.
function streamReply() {
    const encoder = new TextEncoder();
    return new ReadableStream({
        start(controller) {
            const send = data => controller.enqueue(encoder.encode(\`data: \${data}\\n\\n\`));
            const arrive = delta => setTimeout(() => send(JSON.stringify({ choices: [{ delta, index: 0 }] })), 50);
            globalThis.__perfReply = {
                push: content => arrive({ content }),
                think: reasoning_content => arrive({ reasoning_content }),
                end() {
                    send('[DONE]');
                    controller.close();
                    globalThis.__perfReply = null;
                },
            };
        },
    });
}
installWebHost({
    identity: { platform: 'windows', kind: 'desktop' },
    backend: { handle(request) {
        const characters = handleCharacters(request);
        if (characters) return characters;
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
// Host traffic as the native host sees it: the UTF-8 JSON of every command's arguments and result, with binary
// data counted by its bytes. The trace records every call in order for the backend replay; strings the host
// issued (session ids, revisions) become references to the result that issued them, including results that are
// JSON documents in binary form.
globalThis.__perfHost = { bytes: 0, recording: true, step: 'app/startup', calls: [], issued: new Map() };
const utf8 = new TextEncoder();
function hostBytes(value) {
    let binary = 0;
    const text = JSON.stringify(value, (key, item) => {
        if (item instanceof ArrayBuffer || ArrayBuffer.isView(item)) {
            binary += item.byteLength;
            return null;
        }
        return item;
    });
    return (text === undefined ? 0 : utf8.encode(text).length) + binary;
}
const bytesOf = value => value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
const issuedToken = value => typeof value === 'string' && value.length >= 16 && /[0-9]/.test(value) && /[a-z]/i.test(value);
function base64(bytes) {
    let text = '';
    for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(text);
}
function traced(value, issued) {
    if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) return { $bytes: base64(bytesOf(value)) };
    return JSON.parse(JSON.stringify(value ?? {}, (key, item) => {
        if (item instanceof ArrayBuffer || ArrayBuffer.isView(item)) return Array.from(bytesOf(item));
        return issued.has(item) ? { $result: issued.get(item) } : item;
    }));
}
const utf8Text = new TextDecoder();
function parsedResult(value) {
    if (!(value instanceof ArrayBuffer || ArrayBuffer.isView(value))) return value;
    try {
        return JSON.parse(utf8Text.decode(bytesOf(value)));
    } catch {
        return undefined;
    }
}
function rememberIssued(issued, index, value, at = []) {
    if (issuedToken(value) && !issued.has(value)) issued.set(value, [index, at]);
    else if (value && typeof value === 'object' && !ArrayBuffer.isView(value) && !(value instanceof ArrayBuffer)) {
        for (const [key, item] of Object.entries(value)) rememberIssued(issued, index, item, [...at, key]);
    }
}
// Native commands the Web Host lacks, answered as the native host would.
const { session, messages, runs } = data.assistant;
const nativeOnly = {
    count_openai_tokens_batch: ({ dto }) => ({ token_counts: dto.requests.map(request => Math.ceil(JSON.stringify(request.messages).length / 4)) }),
    load_agent_session_profile: () => ({ profile: null }),
    list_agent_sessions: () => ({ sessions: [session], activeRuns: [] }),
    read_agent_session({ dto }) {
        const limit = dto.limit ?? 50;
        const page = messages.filter(message => message.seq < (dto.beforeSeq ?? Infinity)).slice(-limit - 1);
        const more = page.length > limit;
        return { session, messages: more ? page.slice(1) : page, lastSeq: messages.length, nextBeforeSeq: more ? page[1].seq : null, activeRun: null };
    },
    read_agent_run_events({ dto }) {
        const events = runs.find(({ run }) => run.id === dto.runId).events
            .filter(event => event.seq > (dto.afterSeq ?? 0) && event.seq < (dto.beforeSeq ?? Infinity));
        return { events: dto.beforeSeq == null ? events.slice(0, dto.limit) : events.slice(-dto.limit) };
    },
    // The catalog only fills the assistant's settings view.
    list_agent_tools: () => ({ tools: [], diagnostics: [] }),
};
const webHostInvoke = window.__TAURI__.core.invoke;
const invoke = async (command, args, options) => Object.hasOwn(nativeOnly, command)
    ? structuredClone(nativeOnly[command](args))
    : webHostInvoke(command, args, options);
window.__TAURI__.core.invoke = async (command, args, options) => {
    const host = globalThis.__perfHost;
    globalThis.__perfPage.noteActivity();
    if (!host.recording) return invoke(command, args, options);
    host.bytes += hostBytes(args);
    const index = host.calls.length;
    host.calls.push({ step: host.step, command, args: traced(args, host.issued), headers: traced(options?.headers, host.issued) });
    const result = await invoke(command, args, options);
    host.bytes += hostBytes(result);
    rememberIssued(host.issued, index, parsedResult(result));
    return result;
};
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
        const file = pathname === '/__perf/web-host-fixture.mjs' ? fixtureFile : frontendFile(pathname);
        if (!file) throw new Error(`No frontend file at ${pathname}`);
        return { type: CONTENT_TYPES[path.extname(file)] ?? 'application/octet-stream', body: await readFile(file) };
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

// The engine fires these on its own schedule, so their count follows scheduling rather than the page's work:
// slotchange is coalesced per microtask checkpoint; document and resource lifecycle events follow loading;
// content visibility and scroll end follow rendering updates; boundary and legacy focus events follow hit
// testing and focus fixup under a still pointer.
const SCHEDULED_EVENTS = new Set([
    'slotchange',
    'load', 'readystatechange', 'DOMContentLoaded', 'pageshow',
    'contentvisibilityautostatechange', 'scrollend',
    'mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'pointerover', 'pointerout', 'pointerenter', 'pointerleave',
    'DOMFocusIn', 'DOMFocusOut',
]);

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
                counters.eventDispatches += !SCHEDULED_EVENTS.has(event.args?.data?.type);
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
 * records the engine work and host traffic they caused plus the page's size afterwards. With `allocations`, it
 * records only the bytes JavaScript allocated: sampling every allocation slows the page and would change how
 * the engine work splits into frames. `frame()` lets one frame render between inputs.
 */
async function openFrontend({ allocations = false } = {}) {
    const quietWithinMs = allocations ? 30_000 : 5000;
    const server = await serve();
    // Animations, including smooth scrolling, would make frame counts follow elapsed time instead of work.
    const args = ['--disable-smooth-scrolling', '--force-prefers-reduced-motion'];
    // Background compilation would let machine speed decide which allocations optimized code removes.
    if (allocations) args.push('--js-flags=--single-threaded');
    // On Linux, headless Chromium hints web fonts in full by default and rounds their glyph advances to whole
    // pixels, so text wraps differently from macOS; without hinting the advances are the unrounded ones.
    if (process.platform === 'linux') args.push('--font-render-hinting=none');
    // On Linux the harness also issues every frame, which headless Chromium on macOS cannot do. Outside steps a
    // frame comes every 16 ms, as on the engine's own clock; during a step only once the page is quiet, so real
    // time never decides whether work lands before or after a frame.
    const harnessFrames = process.platform === 'linux';
    if (harnessFrames) args.push('--deterministic-mode');
    const browser = await chromium.launch({ args });
    // The page takes its language and time zone from the machine otherwise, and the UI translates and formats
    // dates by them.
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Performance.enable');
    let frameQueue = Promise.resolve(), measuring = false, open = true;
    /** Issues one frame; resolves whether it drew anything. */
    const issueFrame = () => (frameQueue = frameQueue.then(async () =>
        (await cdp.send('HeadlessExperimental.beginFrame', { interval: 1000 / 60 })).hasDamage));
    const realTimeFrames = harnessFrames && (async () => {
        while (open) {
            if (!measuring) await issueFrame();
            await new Promise(resolve => setTimeout(resolve, 16));
        }
    })();
    async function close() {
        open = false;
        await realTimeFrames;
        await browser.close();
        server.close();
    }
    const problems = [];
    page.on('pageerror', error => problems.push(`Page error: ${error.message}`));
    let consoleMessages = 0;
    page.on('console', message => {
        consoleMessages++;
        if (message.type() === 'error') problems.push(`Console error: ${message.text()}`);
    });
    page.on('response', response => { if (response.status() >= 400) problems.push(`HTTP ${response.status()} ${new URL(response.url()).pathname}`); });
    const results = {};
    let requests = 0;
    page.on('request', () => requests++);
    // During a step, network responses are held like longer timeouts: settle() lets them through one at a time.
    let holding = false;
    const held = [];
    await page.route('**/*', route => holding ? held.push(route) : route.continue());
    async function startStep() {
        holding = true;
        await page.evaluate(() => globalThis.__perfPage.startStep());
    }
    async function endStep(name) {
        holding = false;
        const left = await page.evaluate(() => globalThis.__perfPage.endStep());
        if (left.length > 0) problems.push(`${name} left work scheduled after it ended: ${left.join('; ')}`);
    }

    // Waits until the main thread has idled for three frame intervals with no frame or idle callback pending; a
    // pending callback waits for a frame, which the harness issues when it issues frames. Idleness is read from
    // outside the page, so waiting adds no page work.
    async function quiet() {
        const busy = async () => (await cdp.send('Performance.getMetrics')).metrics.find(metric => metric.name === 'TaskDuration').value;
        const deadline = Date.now() + quietWithinMs;
        for (let idle = 0, last = await busy(); idle < 50;) {
            if (Date.now() > deadline) throw new Error(`The page did not go quiet within ${quietWithinMs / 1000} s`);
            await new Promise(resolve => setTimeout(resolve, 10));
            const now = await busy();
            idle = now - last < 0.001 ? idle + 10 : 0;
            last = now;
            if (idle >= 50 && await page.evaluate(() => globalThis.__perfPage.hasPendingCallbacks())) {
                idle = 0;
                if (harnessFrames) await issueFrame();
            }
        }
    }

    // Once the page is quiet, runs the held timeouts, ticks the step's intervals once, lets one held response
    // through, or issues a frame, and repeats until nothing is held and a frame draws nothing. Holding them means
    // no work arrives after the page looks quiet.
    async function settle() {
        for (let ticks = 0, frames = 0, drawn = true; ;) {
            await quiet();
            if (await page.evaluate(() => globalThis.__perfPage.runPendingTimeouts()) > 0) {
                drawn = true;
                continue;
            }
            if (await page.evaluate(() => globalThis.__perfPage.tickIntervals())) {
                if (++ticks > 1000) throw new Error('Intervals keep producing work');
                drawn = true;
                continue;
            }
            const route = held.shift();
            if (route) {
                await route.continue();
                await (await route.request().response())?.finished();
                drawn = true;
                continue;
            }
            if (!harnessFrames || !drawn) return;
            if (++frames > 1000) throw new Error('Frames keep drawing');
            drawn = await issueFrame();
        }
    }

    /** One frame between inputs, as a typist's keys render one by one; harness frames come once the page is quiet. */
    async function frame() {
        if (!harnessFrames) return page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve))));
        await quiet();
        await issueFrame();
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

    // Settling once more must find nothing: work arriving after the page looked quiet came through a path the
    // harness does not hold, and would land in this step or the next depending on timing.
    async function run(name, inputs) {
        for (const input of inputs) {
            await input();
            await settle();
        }
        const activity = () => page.evaluate(() => globalThis.__perfPage.activity());
        const before = await activity();
        await settle();
        const late = await activity() - before;
        if (late > 0) problems.push(`${name} kept working after the page looked quiet: ${late} more timer, callback or host call runs`);
    }

    // Every allocation in the step counts once, including objects already collected, except allocations whose
    // nearest script is the page's harness, the Web Host or its fixture: they stand in for the native host.
    async function allocatedBytes(step) {
        await cdp.send('HeapProfiler.startSampling', {
            samplingInterval: 1, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true,
        });
        await step();
        const { profile } = await cdp.send('HeapProfiler.stopSampling');
        const parents = new Map(), frames = new Map();
        (function index(node, parent) {
            frames.set(node.id, node.callFrame);
            if (parent) parents.set(node.id, parent.id);
            for (const child of node.children) index(child, node);
        })(profile.head, null);
        const standIn = id => {
            for (; id !== undefined; id = parents.get(id)) {
                const { url } = frames.get(id);
                if (url) {
                    const { pathname } = new URL(url);
                    return pathname === '/' || pathname.startsWith('/tauri/web-host/') || pathname.startsWith('/__perf/');
                }
            }
            return false;
        };
        return profile.samples.reduce((sum, sample) => standIn(sample.nodeId) ? sum : sum + sample.size, 0);
    }

    const hostBytes = () => page.evaluate(() => globalThis.__perfHost.bytes);
    const traceStep = step => page.evaluate(value => { globalThis.__perfHost.step = value; }, step);

    async function measure(name, ...inputs) {
        if (name in results) throw new Error(`Duplicate perf step: ${name}`);
        measuring = true;
        try {
            await settle();
            if (allocations) {
                await startStep();
                results[name] = { jsAllocatedBytes: await allocatedBytes(() => run(name, inputs)) };
                await endStep(name);
                return;
            }
            const before = requests, messagesBefore = consoleMessages, hostBefore = await hostBytes();
            await traceStep(name);
            await startStep();
            const work = await trace(() => run(name, inputs));
            await endStep(name);
            await traceStep(null);
            results[name] = {
                ...work, requests: requests - before, consoleMessages: consoleMessages - messagesBefore,
                hostBytes: await hostBytes() - hostBefore, ...await pageSize(),
            };
        } finally {
            measuring = false;
        }
    }

    // Startup's work depends on when resources finish loading, so it records the page it produces.
    try {
        // index.html declares <base href="/">: with a query string, jQuery UI Tabs treats its tabs as remote and
        // loads the whole page into a tab panel.
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        // The fixture's character opens its chat, review-chat, once startup has finished.
        await page.waitForFunction(() => globalThis.__TAURITAVERN_STARTUP_STAGE__ === 'full'
            && globalThis.SillyTavern?.getContext().chatId === 'review-chat', null, { polling: 100 });
        // The engine hit-tests a known pointer whenever layout changes, and some platforms start with one, so
        // the pointer starts at a fixed point.
        await page.mouse.move(1, 1);
        await settle();
        if (allocations) {
            await page.evaluate(() => { globalThis.__perfHost.recording = false; });
            await cdp.send('HeapProfiler.enable');
        } else {
            await traceStep(null);
            results['app/startup'] = await pageSize();
        }
    } catch (error) {
        await close();
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
        frame,
        centerOf,
        results,
        problems,
        chromium: browser.version(),
        /** Every host call so far, in order, with the step it belongs to: `app/startup`, a measured step or null. */
        hostCalls: () => page.evaluate(() => globalThis.__perfHost.calls),
        close,
    };
}

async function runScenarios(options) {
    const directory = new URL('./', import.meta.url);
    const app = await openFrontend(options);
    try {
        for (const file of (await readdir(directory)).filter(name => name.endsWith('.perf.mjs')).sort()) {
            const { default: scenario } = await import(new URL(file, directory));
            await scenario(app);
        }
        app.calls = await app.hostCalls();
    } finally {
        await app.close();
    }
    if (app.problems.length > 0) {
        throw new Error(`The frontend reported problems, so its counters are not comparable:\n${app.problems.join('\n')}`);
    }
    return app;
}

/**
 * Runs every *.perf.mjs scenario in two sessions, one for engine work and host traffic and one for JavaScript
 * allocations. Returns each step's counters, the browser version and the first session's host calls.
 */
export async function measureFrontend() {
    const engine = await runScenarios();
    const allocations = (await runScenarios({ allocations: true })).results;
    for (const step of Object.keys(engine.results)) {
        if (step === 'app/startup') continue;
        if (!allocations[step]) throw new Error(`The allocation session did not measure ${step}`);
        Object.assign(engine.results[step], allocations[step]);
    }
    return { chromium: engine.chromium, steps: engine.results, calls: engine.calls };
}
