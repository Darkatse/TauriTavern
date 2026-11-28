import { expect, test } from '@rstest/core';
import { createObservation, interact } from '@tauritavern/app-use';
import { createInteractionOptions } from '../../src/scripts/extensions/in-app-agent/src/tools.ts';
import { importFrontend } from '../helpers/frontend.mjs';

const { parkManagedIframe, dropParkedManagedIframe } = await importFrontend('tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js');
const { initDrawers, toggleInlineDrawer } = await importFrontend('scripts/drawers.js');
const { initLegacyControls } = await importFrontend('scripts/legacy-controls.js');

const loaded = frame => new Promise(resolve => frame.addEventListener('load', resolve, { once: true }));
const interactionOptions = createInteractionOptions(() => { throw new Error('Nested #chat must not use the host scroller'); });

function ref(page, name) {
    const line = page.tree.split('\n').find(line => line.includes(`[ref=`) && line.includes(`"${name}"`));
    const value = line?.match(/\[ref=([^\]]+)\]/)?.[1];
    if (!value) throw new Error(`Missing ${name}: ${page.tree}`);
    return value;
}

function throws(action, message) {
    try { action(); } catch (error) { return error; }
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

test('app-use enters frames explicitly, respects outer obstruction, follows navigation and keeps parked pages out', async () => {
    const fixture = document.createElement('div');
    fixture.style.cssText = 'position:fixed;left:12px;top:100px;width:calc(100% - 24px);height:420px;background:white';
    document.body.append(fixture);
    const observation = createObservation();
    observation.enterRun('iframe-check');
    const main = () => observation.snapshot({ depth: 6 });
    const enter = (page, name) => observation.snapshot({ root: ref(page, name), depth: 6 });
    const act = (page, name, args) => interact({ ...args, ref: ref(page, name) }, observation, interactionOptions);
    try {
        const frame = await mountFrame(fixture, 'Embedded form', `<!doctype html>
            <section aria-label="Child form">
            <label>Child text<input oninput="document.querySelector('output').value=this.value"></label>
            <button onclick="this.textContent='Clicked'">Child action</button>
            <output>before</output></section>`);
        const overview = main();
        expect(!overview.tree.includes('Child text'), 'overview entered a frame implicitly').toBeTruthy();
        const filled = interact({ action: 'fill', root: ref(overview, 'Embedded form'), selector: 'input', value: 'changed' }, observation, interactionOptions);
        expect(filled.observed.connected, 'selector readback used the main document for an iframe target').toBeTruthy();
        const page = enter(overview, 'Embedded form');
        const child = frame.contentDocument;
        expect(child.querySelector('output').value === 'changed', 'child did not receive the input event').toBeTruthy();
        // ST's compositor workaround also changes fixed positioning; scope it to this scenario.
        document.documentElement.style.cssText = '-webkit-transform:translateZ(0);-webkit-backface-visibility:hidden;-webkit-perspective:1000';
        try {
            const clicked = act(page, 'Child action', { action: 'click' });
            expect(clicked.dispatched && child.querySelector('button').textContent === 'Clicked', 'ST root perspective prevented the iframe click').toBeTruthy();
        } finally { document.documentElement.style.cssText = ''; }

        const childRef = ref(page, 'Child text');
        const childFormRef = ref(page, 'Child form');
        const overlay = document.createElement('div');
        overlay.id = 'app-use-blocker';
        overlay.style.cssText = 'position:fixed;inset:0;background:#ffffff01;z-index:99999';
        document.body.append(overlay);
        try {
            const error = throws(() => interact({ action: 'fill', root: childFormRef, selector: 'input', value: 'blocked' }, observation, interactionOptions), 'outer overlay was bypassed');
            expect(error.message.includes('app-use-blocker') && error.message.includes('main page'), 'diagnostic did not identify the outer obstruction').toBeTruthy();
            expect(child.querySelector('output').value === 'changed', 'blocked action changed the child').toBeTruthy();
        }
        finally { overlay.remove(); }
        frame.style.transform = 'translateY(-1000px)';
        try {
            const error = throws(() => interact({ action: 'fill', ref: childRef, value: 'outside' }, observation, interactionOptions), 'offscreen frame was operated');
            expect(error.message.includes('outside the visible area') && error.message.includes('main page'), 'offscreen diagnostic lost the containing page').toBeTruthy();
            expect(child.querySelector('output').value === 'changed', 'offscreen action changed the child').toBeTruthy();
        } finally { frame.style.transform = 'scale(.85)'; }
        act(page, 'Child text', { action: 'fill', value: 'recovered' });
        expect(child.querySelector('output').value === 'recovered', 'action did not recover after removing the overlay').toBeTruthy();

        const adopted = child.createElement('input');
        adopted.setAttribute('aria-label', 'Adopted input');
        fixture.prepend(adopted);
        const mainResult = interact({ action: 'fill', selector: 'input[aria-label="Adopted input"]', value: 'adopted' }, observation, interactionOptions);
        expect(adopted.value === 'adopted' && mainResult.observed.connected, 'unscoped selector did not use the main document').toBeTruthy();
        act(page, 'Child text', { action: 'fill', value: 'after main-page action' });
        expect(child.querySelector('output').value === 'after main-page action', 'selector changed the existing snapshot scope').toBeTruthy();

        initDrawers();
        initLegacyControls();
        const drawer = child.createElement('div');
        drawer.className = 'inline-drawer';
        drawer.innerHTML = '<div class="inline-drawer-toggle"><i class="inline-drawer-icon down"></i></div><div class="inline-drawer-content" style="display:none">Adopted panel</div>';
        fixture.prepend(drawer);
        expect(drawer.ownerDocument === document && !(drawer instanceof HTMLElement), 'fixture must retain the iframe prototype after adoption').toBeTruthy();
        await new Promise(requestAnimationFrame);
        const icon = drawer.querySelector('.inline-drawer-icon');
        const content = drawer.querySelector('.inline-drawer-content');
        expect(document.getElementById(icon.getAttribute('aria-controls')) === content && icon.tabIndex === 0, 'adopted drawer was not discovered').toBeTruthy();
        icon.addEventListener('click', () => toggleInlineDrawer(drawer, 0));
        icon.click();
        expect(content.style.display === 'block' && icon.getAttribute('aria-expanded') === 'true', 'adopted drawer did not open').toBeTruthy();
        icon.focus();
        icon.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
        expect(content.style.display === 'none' && icon.getAttribute('aria-expanded') === 'false', 'adopted drawer did not close through keyboard activation').toBeTruthy();
        icon.classList.replace('down', 'up');
        content.style.display = 'flex';
        await new Promise(requestAnimationFrame);
        expect(icon.getAttribute('aria-expanded') === 'true' && content.style.display === 'flex', 'adopted drawer lost external state projection').toBeTruthy();
        drawer.remove();

        const wrapper = document.createElement('div');
        wrapper.style.cssText = 'width:0;height:0';
        wrapper.innerHTML = `<iframe title="Zero frame" style="width:0;height:0;border:0"></iframe>
            <button style="position:fixed;left:20px;top:70px">Floating child</button>`;
        fixture.append(wrapper);
        const layout = main();
        expect(!layout.tree.includes('Zero frame') && layout.tree.includes('Floating child'), 'zero-size filtering removed a visible descendant or exposed an empty frame').toBeTruthy();
        wrapper.remove();

        child.body.replaceChildren();
        const nested = await mountFrame(child.body, 'Nested page', `<!doctype html><style>body{margin:0}#chat{height:60px;overflow:auto}</style>
            <input aria-label="Nested text"><div id="chat" role="region" aria-label="Local scroller"><div style="height:400px">Content</div></div>`, { blob: true });
        const outer = enter(main(), 'Embedded form');
        let inner = observation.snapshot({ root: ref(outer, 'Nested page'), selector: 'body', depth: 6 });
        act(inner, 'Nested text', { action: 'fill', value: 'nested' });
        const oldInput = nested.contentDocument.querySelector('input');
        expect(oldInput.value === 'nested', 'nested input failed').toBeTruthy();
        act(inner, 'Local scroller', { action: 'scroll', direction: 'down' });
        expect(nested.contentDocument.querySelector('#chat').scrollTop > 0, 'nested #chat used the host scroller').toBeTruthy();

        nested.contentDocument.body.insertAdjacentHTML('beforeend', '<button>Many</button>'.repeat(100));
        inner = enter(enter(main(), 'Embedded form'), 'Nested page');
        const oldRef = ref(inner, 'Nested text');
        const cursor = inner.nextCursor;
        expect(cursor, 'fixture did not create a continuation').toBeTruthy();
        const reload = loaded(nested);
        nested.srcdoc = '<input aria-label="Replacement">';
        await reload;
        throws(() => observation.captureLayout({ marks: true }), 'navigated snapshot silently lost all screenshot marks');
        throws(() => interact({ action: 'fill', ref: oldRef, value: 'stale' }, observation, interactionOptions), 'old document ref remained actionable');
        expect(oldInput.value === 'nested', 'old document was mutated').toBeTruthy();
        throws(() => observation.snapshot({ cursor }), 'old document cursor remained readable');
        const replacement = enter(enter(main(), 'Embedded form'), 'Nested page');
        act(replacement, 'Replacement', { action: 'fill', value: 'recovered' });
        const replacementInput = nested.contentDocument.querySelector('input');
        expect(replacementInput.value === 'recovered', 'fresh observation did not recover after navigation').toBeTruthy();
        const replacementRef = ref(replacement, 'Replacement');
        const reloadOuter = loaded(frame);
        frame.srcdoc = '<p>Outer replacement</p>';
        await reloadOuter;
        throws(() => interact({ action: 'fill', ref: replacementRef, value: 'stale' }, observation, interactionOptions), 'ancestor reload left a live ref');
        expect(replacementInput.value === 'recovered', 'ancestor reload allowed an old document mutation').toBeTruthy();

        const parked = await mountFrame(fixture, 'Parked page', '<input aria-label="Parked input">');
        const parkedRef = ref(enter(main(), 'Parked page'), 'Parked input');
        parkManagedIframe({ id: 'app-use-check', iframe: parked, maxIframes: 1, ttlMs: 60_000 });
        throws(() => observation.captureLayout({ marks: true }), 'parked snapshot silently lost all screenshot marks');
        throws(() => interact({ action: 'fill', ref: parkedRef, value: 'hidden' }, observation, interactionOptions), 'parked page remained actionable');
        expect(!main().tree.includes('Parked page'), 'parked page leaked into observation').toBeTruthy();
        act(main(), 'Adopted input', { action: 'fill', value: 'still works' });
        expect(adopted.value === 'still works', 'frame failures prevented main-page operations').toBeTruthy();
    } finally {
        dropParkedManagedIframe('app-use-check');
    }
});
