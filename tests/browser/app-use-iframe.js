import { createObservation } from '../../src/scripts/extensions/in-app-agent/src/ui/snapshot.ts';
import { interact } from '../../src/scripts/extensions/in-app-agent/src/ui/interact.ts';
import { parkManagedIframe, dropParkedManagedIframe } from '../../src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js';

const assert = (condition, message) => { if (!condition) throw new Error(message); };
const loaded = frame => new Promise(resolve => frame.addEventListener('load', resolve, { once: true }));
const signal = new AbortController().signal;
const fixture = document.querySelector('#fixture');
const result = document.querySelector('#result');
const run = document.querySelector('#run');
fixture.style.cssText = 'position:fixed;left:12px;top:100px;width:calc(100% - 24px);height:420px;background:white';

function ref(page, name) {
    const line = page.tree.split('\n').find(line => line.includes(`[ref=`) && line.includes(`"${name}"`));
    const value = line?.match(/\[ref=([^\]]+)\]/)?.[1];
    if (!value) throw new Error(`Missing ${name}: ${page.tree}`);
    return value;
}

async function rejected(action, message) {
    try { await action(); } catch (error) { return error; }
    throw new Error(message);
}

async function mountFrame(parent, title, html, { blob = false } = {}) {
    const frame = parent.ownerDocument.createElement('iframe');
    frame.title = title;
    frame.style.cssText = 'width:90%;height:300px;border:4px solid gray;padding:6px;transform:scale(.85);transform-origin:top left';
    const url = blob ? URL.createObjectURL(new Blob([html], { type: 'text/html' })) : null;
    if (url) frame.src = url;
    else frame.srcdoc = html;
    const ready = loaded(frame);
    parent.append(frame);
    await ready;
    if (url) URL.revokeObjectURL(url);
    return frame;
}

run.onclick = async () => {
    run.disabled = true;
    result.textContent = 'Running…';
    fixture.replaceChildren();
    const reports = [];
    const observation = createObservation();
    observation.enterRun('iframe-check');
    const main = () => observation.snapshot({ depth: 6 });
    const enter = (page, name) => observation.snapshot({ root: ref(page, name), depth: 6 });
    const act = (page, name, args) => interact({ ...args, ref: ref(page, name) }, observation, signal);
    try {
        const frame = await mountFrame(fixture, 'Embedded form', `<!doctype html>
            <section aria-label="Child form">
            <label>Child text<input oninput="document.querySelector('output').value=this.value"></label>
            <output>before</output></section>`);
        const overview = main();
        assert(!overview.tree.includes('Child text'), 'overview entered a frame implicitly');
        const filled = await interact({ action: 'fill', root: ref(overview, 'Embedded form'), selector: 'input', value: 'changed' }, observation, signal);
        assert(filled.observed.connected, 'selector readback used the main document for an iframe target');
        const page = enter(overview, 'Embedded form');
        const child = frame.contentDocument;
        assert(child.querySelector('output').value === 'changed', 'child did not receive the input event');

        const childRef = ref(page, 'Child text');
        const childFormRef = ref(page, 'Child form');
        const overlay = document.createElement('div');
        overlay.id = 'app-use-blocker';
        overlay.style.cssText = 'position:fixed;inset:0;background:#ffffff01;z-index:99999';
        document.body.append(overlay);
        try {
            const error = await rejected(() => interact({ action: 'fill', root: childFormRef, selector: 'input', value: 'blocked' }, observation, signal), 'outer overlay was bypassed');
            assert(error.message.includes('app-use-blocker') && error.message.includes('main page'), 'diagnostic did not identify the outer obstruction');
            assert(child.querySelector('output').value === 'changed', 'blocked action changed the child');
        }
        finally { overlay.remove(); }
        frame.style.transform = 'translateY(-1000px)';
        try {
            const error = await rejected(() => interact({ action: 'fill', ref: childRef, value: 'outside' }, observation, signal), 'offscreen frame was operated');
            assert(error.message.includes('outside the visible area') && error.message.includes('main page'), 'offscreen diagnostic lost the containing page');
            assert(child.querySelector('output').value === 'changed', 'offscreen action changed the child');
        } finally { frame.style.transform = 'scale(.85)'; }
        await act(page, 'Child text', { action: 'fill', value: 'recovered' });
        assert(child.querySelector('output').value === 'recovered', 'action did not recover after removing the overlay');
        reports.push('PASS: explicit entry; outer obstruction prevents mutation and permits recovery');

        const adopted = child.createElement('input');
        adopted.setAttribute('aria-label', 'Adopted input');
        fixture.prepend(adopted);
        const mainResult = await interact({ action: 'fill', selector: 'input[aria-label="Adopted input"]', value: 'adopted' }, observation, signal);
        assert(adopted.value === 'adopted' && mainResult.observed.connected, 'unscoped selector did not use the main document');
        await act(page, 'Child text', { action: 'fill', value: 'after main-page action' });
        assert(child.querySelector('output').value === 'after main-page action', 'selector changed the existing snapshot scope');

        const wrapper = document.createElement('div');
        wrapper.style.cssText = 'width:0;height:0';
        wrapper.innerHTML = `<iframe title="Zero frame" style="width:0;height:0;border:0"></iframe>
            <button style="position:fixed;left:20px;top:70px">Floating child</button>`;
        fixture.append(wrapper);
        const layout = main();
        assert(!layout.tree.includes('Zero frame') && layout.tree.includes('Floating child'), 'zero-size filtering removed a visible descendant or exposed an empty frame');
        wrapper.remove();

        child.body.replaceChildren();
        const nested = await mountFrame(child.body, 'Nested page', `<!doctype html><style>body{margin:0}#chat{height:60px;overflow:auto}</style>
            <input aria-label="Nested text"><div id="chat" role="region" aria-label="Local scroller"><div style="height:400px">Content</div></div>`, { blob: true });
        const outer = enter(main(), 'Embedded form');
        let inner = observation.snapshot({ root: ref(outer, 'Nested page'), selector: 'body', depth: 6 });
        await act(inner, 'Nested text', { action: 'fill', value: 'nested' });
        const oldInput = nested.contentDocument.querySelector('input');
        assert(oldInput.value === 'nested', 'nested input failed');
        await act(inner, 'Local scroller', { action: 'scroll', direction: 'down' });
        assert(nested.contentDocument.querySelector('#chat').scrollTop > 0, 'nested #chat used the host scroller');
        reports.push('PASS: cross-window controls, nested Blob document and local #chat scrolling');

        nested.contentDocument.body.insertAdjacentHTML('beforeend', '<button>Many</button>'.repeat(100));
        inner = enter(enter(main(), 'Embedded form'), 'Nested page');
        const oldRef = ref(inner, 'Nested text');
        const cursor = inner.nextCursor;
        assert(cursor, 'fixture did not create a continuation');
        const reload = loaded(nested);
        nested.srcdoc = '<input aria-label="Replacement">';
        await reload;
        await rejected(() => interact({ action: 'fill', ref: oldRef, value: 'stale' }, observation, signal), 'old document ref remained actionable');
        assert(oldInput.value === 'nested', 'old document was mutated');
        await rejected(() => observation.snapshot({ cursor }), 'old document cursor remained readable');
        const replacement = enter(enter(main(), 'Embedded form'), 'Nested page');
        await act(replacement, 'Replacement', { action: 'fill', value: 'recovered' });
        const replacementInput = nested.contentDocument.querySelector('input');
        assert(replacementInput.value === 'recovered', 'fresh observation did not recover after navigation');
        const replacementRef = ref(replacement, 'Replacement');
        const reloadOuter = loaded(frame);
        frame.srcdoc = '<p>Outer replacement</p>';
        await reloadOuter;
        await rejected(() => interact({ action: 'fill', ref: replacementRef, value: 'stale' }, observation, signal), 'ancestor reload left a live ref');
        assert(replacementInput.value === 'recovered', 'ancestor reload allowed an old document mutation');
        reports.push('PASS: frame/ancestor navigation rejects old refs and cursor; re-observation recovers');

        const parked = await mountFrame(fixture, 'Parked page', '<input aria-label="Parked input">');
        const parkedRef = ref(enter(main(), 'Parked page'), 'Parked input');
        parkManagedIframe({ id: 'app-use-check', iframe: parked, maxIframes: 1, ttlMs: 60_000 });
        await rejected(() => interact({ action: 'fill', ref: parkedRef, value: 'hidden' }, observation, signal), 'parked page remained actionable');
        assert(!main().tree.includes('Parked page'), 'parked page leaked into observation');
        await act(main(), 'Adopted input', { action: 'fill', value: 'still works' });
        assert(adopted.value === 'still works', 'frame failures prevented main-page operations');
        reports.push('PASS: parking stays outside the UI; main-page operations remain available');
        result.textContent = reports.join('\n');
    } catch (error) {
        result.textContent = [...reports, `FAIL: ${error.stack ?? error}`].join('\n');
    } finally {
        dropParkedManagedIframe('app-use-check');
        fixture.replaceChildren();
        run.disabled = false;
    }
};
