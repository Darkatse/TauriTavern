import test from 'node:test';
import assert from 'node:assert/strict';
import { Window } from 'happy-dom';

let instance = 0;
async function setup(t, html) {
    const window = new Window();
    const globals = {
        document: window.document, Element: window.Element, HTMLElement: window.HTMLElement,
        MutationObserver: window.MutationObserver, CustomEvent: window.CustomEvent,
        getComputedStyle: window.getComputedStyle.bind(window),
    };
    const previous = new Map(Object.keys(globals).map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
    Object.assign(globalThis, globals);
    t.after(async () => {
        await window.happyDOM.abort();
        window.close();
        for (const [name, descriptor] of previous) {
            if (descriptor) Object.defineProperty(globalThis, name, descriptor);
            else delete globalThis[name];
        }
    });
    window.document.body.innerHTML = html;
    const drawers = await import(`../src/scripts/drawers.js?test=${++instance}`);
    return { ...globals, drawers, flush: () => window.happyDOM.waitUntilComplete() };
}

function controlMotion(content) {
    const animations = [];
    content.animate = () => {
        const animation = {
            onfinish: null, oncancel: null,
            cancel() { this.oncancel?.(); },
            finish() { this.onfinish?.(); },
        };
        animations.push(animation);
        return animation;
    };
    return animations;
}

const inlineMarkup = `<div class="inline-drawer">
    <div class="inline-drawer-header inline-drawer-toggle"><b>Settings</b><i class="inline-drawer-icon down"></i></div>
    <div class="inline-drawer-content" style="display:none"><input></div>
</div>`;

test('top-level changes publish once through core and external writes, including a moved panel', async t => {
    const { document, drawers, flush } = await setup(t, `<div class="drawer">
        <div class="drawer-toggle" title="Settings"><i class="drawer-icon closedIcon"></i></div>
        <div id="settings" class="drawer-content closedDrawer"></div>
    </div><div id="destination"></div>`);
    const panel = document.getElementById('settings');
    const toggle = document.querySelector('.drawer-toggle');
    const icon = document.querySelector('.drawer-icon');
    const states = [];
    const unsubscribe = drawers.subscribeDrawerState(panel, () => {
        states.push({ open: drawers.isTopLevelDrawerOpen(panel), expanded: icon.getAttribute('aria-expanded') });
    });
    drawers.initDrawers();
    drawers.initDrawers();
    assert.equal(icon.getAttribute('aria-controls'), panel.id);
    drawers.setTopLevelDrawerOpen(panel, true);
    assert.deepEqual(states, [{ open: true, expanded: 'true' }]);
    assert.equal(panel.classList.contains('closedDrawer'), false);
    await flush();
    assert.equal(states.length, 1);

    panel.classList.remove('openDrawer');
    panel.classList.add('closedDrawer');
    panel.style.display = 'none';
    await flush();
    assert.deepEqual(states.at(-1), { open: false, expanded: 'false' });
    assert.equal(states.length, 2);
    assert.equal(panel.style.display, 'none');

    document.getElementById('destination').append(panel);
    await flush();
    drawers.setTopLevelDrawerOpen(drawers.getTopLevelDrawerPanel(toggle), true);
    drawers.setTopLevelDrawerOpen(panel, true);
    await flush();
    assert.equal(states.length, 3);
    assert.equal(panel.style.display, '');
    assert.equal(icon.classList.contains('openIcon'), true);
    unsubscribe();
    drawers.setTopLevelDrawerOpen(panel, false);
    assert.equal(states.length, 3);
});

test('animated and public instant toggles retain their distinct lazy-editor notification order', async t => {
    const { document, drawers } = await setup(t, inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const icon = root.querySelector('.inline-drawer-icon');
    const animations = controlMotion(content);
    const notifications = [];
    const completed = [];
    root.addEventListener('inline-drawer-toggle', event => notifications.push({
        detail: event.detail, display: content.style.display,
        open: drawers.isInlineDrawerOpen(root), expanded: icon.getAttribute('aria-expanded'),
    }));
    root.addEventListener('inline-drawer-motion-complete', event => completed.push(event.detail.open));

    assert.equal(drawers.toggleInlineDrawer(root, 160), true);
    assert.deepEqual(notifications, [{ detail: { open: true }, display: 'none', open: true, expanded: 'true' }]);
    assert.equal(content.style.display, 'block');
    animations[0].finish();
    await Promise.resolve();
    assert.deepEqual(completed, [true]);

    drawers.setInlineDrawerOpen(root, false);
    assert.deepEqual(notifications.at(-1), { detail: null, display: 'none', open: false, expanded: 'false' });
    drawers.setInlineDrawerOpen(root, true);
    assert.deepEqual(notifications.at(-1), { detail: null, display: 'block', open: true, expanded: 'true' });
    await Promise.resolve();
    assert.deepEqual(completed, [true]);
});

test('a synchronous toggle listener can close the drawer before opening motion starts', async t => {
    const { document, drawers } = await setup(t, inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const icon = root.querySelector('.inline-drawer-icon');
    controlMotion(content);
    const completed = [];
    root.addEventListener('inline-drawer-toggle', event => {
        if (event.detail?.open) drawers.setInlineDrawerOpen(root, false);
    });
    root.addEventListener('inline-drawer-motion-complete', event => completed.push(event.detail.open));

    assert.equal(drawers.toggleInlineDrawer(root, 160), false);
    assert.equal(drawers.isInlineDrawerOpen(root), false);
    assert.equal(icon.getAttribute('aria-expanded'), 'false');
    assert.equal(content.style.display, 'none');
    await Promise.resolve();
    assert.deepEqual(completed, []);
});

test('rapid animated and instant changes cannot commit or announce obsolete motion', async t => {
    const { document, drawers } = await setup(t, inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const animations = controlMotion(content);
    const completed = [];
    root.addEventListener('inline-drawer-motion-complete', event => completed.push(event.detail.open));
    drawers.toggleInlineDrawer(root, 160);
    const staleOpenFinish = animations[0].onfinish;
    drawers.toggleInlineDrawer(root, 160);
    const staleCloseFinish = animations[1].onfinish;
    assert.equal(drawers.isInlineDrawerOpen(root), false);
    assert.equal(content.style.display, 'block');
    drawers.setInlineDrawerOpen(root, true);
    staleOpenFinish();
    staleCloseFinish();
    await Promise.resolve();
    assert.equal(content.style.display, 'block');
    assert.deepEqual(completed, []);

    drawers.toggleInlineDrawer(root, 0);
    drawers.setInlineDrawerOpen(root, true);
    await Promise.resolve();
    assert.deepEqual(completed, []);
    drawers.toggleInlineDrawer(root, 160);
    animations.at(-1).finish();
    await Promise.resolve();
    assert.equal(content.style.display, 'none');
    assert.deepEqual(completed, [false]);
});

test('external direction classes project without replaying notifications or fighting extension display', async t => {
    const { document, drawers, flush, CustomEvent } = await setup(t, inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const icon = root.querySelector('.inline-drawer-icon');
    const animations = controlMotion(content);
    drawers.initDrawers();
    const lazyStates = [];
    let completed = 0;
    root.addEventListener('inline-drawer-toggle', () => lazyStates.push(drawers.isInlineDrawerOpen(root)));
    root.addEventListener('inline-drawer-motion-complete', () => completed++);
    icon.classList.replace('down', 'up');
    root.dispatchEvent(new CustomEvent('inline-drawer-toggle', { bubbles: true }));
    assert.deepEqual(lazyStates, [true]);
    assert.equal(content.style.display, 'none');
    content.style.display = 'flex';
    await flush();
    assert.equal(icon.getAttribute('aria-expanded'), 'true');
    assert.deepEqual(lazyStates, [true]);

    drawers.toggleInlineDrawer(root, 160);
    const staleFinish = animations[0].onfinish;
    icon.classList.remove('down');
    icon.classList.add('up');
    content.style.display = 'flex';
    await flush();
    staleFinish();
    await Promise.resolve();
    assert.equal(content.style.display, 'flex');
    assert.equal(icon.getAttribute('aria-expanded'), 'true');
    assert.equal(completed, 0);
    assert.deepEqual(lazyStates, [true, false]);
});

test('cloned instances get their own associations and native or nested toggles receive projection', async t => {
    const { document, drawers, flush } = await setup(t, `<div class="template_element">${inlineMarkup}</div>
        <div id="native" class="inline-drawer"><button type="button" class="inline-drawer-toggle" aria-label="Settings">
            <i class="inline-drawer-icon down"></i></button><div class="inline-drawer-content" style="display:none"></div></div>
        <div id="nested" class="inline-drawer"><div class="controls"><button class="maximize">Maximize</button>
            <div class="inline-drawer-toggle"><i class="inline-drawer-icon up"></i></div></div>
            <div class="inline-drawer-content" style="display:flex"></div></div>`);
    drawers.initDrawers();
    const template = document.querySelector('.template_element .inline-drawer');
    const first = template.cloneNode(true);
    const second = template.cloneNode(true);
    document.body.append(first, second);
    await flush();
    for (const root of [first, second]) {
        const icon = root.querySelector('.inline-drawer-icon');
        const content = root.querySelector('.inline-drawer-content');
        assert.equal(document.getElementById(icon.getAttribute('aria-controls')), content);
        assert.equal(root.querySelector('.inline-drawer-header').hasAttribute('aria-expanded'), false);
    }
    const native = document.getElementById('native');
    drawers.setInlineDrawerOpen(native, true);
    assert.equal(native.querySelector('button').getAttribute('aria-expanded'), 'true');
    assert.equal(native.querySelector('i').hasAttribute('aria-expanded'), false);
    const nested = document.getElementById('nested');
    assert.equal(nested.querySelector('.inline-drawer-icon').getAttribute('aria-expanded'), 'true');
    drawers.setInlineDrawerOpen(nested, false);
    assert.equal(nested.querySelector('.inline-drawer-icon').getAttribute('aria-expanded'), 'false');
    assert.equal(nested.querySelector('.maximize').hasAttribute('aria-expanded'), false);
});
