import { beforeAll, expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const drawers = await importFrontend('scripts/drawers.js');

beforeAll(() => drawers.initDrawers());

const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));

async function render(html) {
    document.body.innerHTML = html;
    await nextTask();
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

test('top-level changes publish once through core and external writes, including a moved panel', async () => {
    await render(`<div class="drawer">
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
    expect(icon.getAttribute('aria-controls')).toBe(panel.id);
    drawers.setTopLevelDrawerOpen(panel, true);
    expect(states).toEqual([{ open: true, expanded: 'true' }]);
    expect(panel.classList.contains('closedDrawer')).toBe(false);
    await nextTask();
    expect(states.length).toBe(1);

    panel.classList.remove('openDrawer');
    panel.classList.add('closedDrawer');
    panel.style.display = 'none';
    await nextTask();
    expect(states.at(-1)).toEqual({ open: false, expanded: 'false' });
    expect(states.length).toBe(2);
    expect(panel.style.display).toBe('none');

    document.getElementById('destination').append(panel);
    await nextTask();
    drawers.setTopLevelDrawerOpen(drawers.getTopLevelDrawerPanel(toggle), true);
    drawers.setTopLevelDrawerOpen(panel, true);
    await nextTask();
    expect(states.length).toBe(3);
    expect(panel.style.display).toBe('');
    expect(icon.classList.contains('openIcon')).toBe(true);
    unsubscribe();
    drawers.setTopLevelDrawerOpen(panel, false);
    expect(states.length).toBe(3);
});

test('animated and public instant toggles retain their distinct lazy-editor notification order', async () => {
    await render(inlineMarkup);
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

    expect(drawers.toggleInlineDrawer(root, 160)).toBe(true);
    expect(notifications).toEqual([{ detail: { open: true }, display: 'none', open: true, expanded: 'true' }]);
    expect(content.style.display).toBe('block');
    animations[0].finish();
    await Promise.resolve();
    expect(completed).toEqual([true]);

    drawers.setInlineDrawerOpen(root, false);
    expect(notifications.at(-1)).toEqual({ detail: null, display: 'none', open: false, expanded: 'false' });
    drawers.setInlineDrawerOpen(root, true);
    expect(notifications.at(-1)).toEqual({ detail: null, display: 'block', open: true, expanded: 'true' });
    await Promise.resolve();
    expect(completed).toEqual([true]);
});

test('a synchronous toggle listener can close the drawer before opening motion starts', async () => {
    await render(inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const icon = root.querySelector('.inline-drawer-icon');
    controlMotion(content);
    const completed = [];
    root.addEventListener('inline-drawer-toggle', event => {
        if (event.detail?.open) drawers.setInlineDrawerOpen(root, false);
    });
    root.addEventListener('inline-drawer-motion-complete', event => completed.push(event.detail.open));

    expect(drawers.toggleInlineDrawer(root, 160)).toBe(false);
    expect(drawers.isInlineDrawerOpen(root)).toBe(false);
    expect(icon.getAttribute('aria-expanded')).toBe('false');
    expect(content.style.display).toBe('none');
    await Promise.resolve();
    expect(completed).toEqual([]);
});

test('rapid animated and instant changes cannot commit or announce obsolete motion', async () => {
    await render(inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const animations = controlMotion(content);
    const completed = [];
    root.addEventListener('inline-drawer-motion-complete', event => completed.push(event.detail.open));
    drawers.toggleInlineDrawer(root, 160);
    const staleOpenFinish = animations[0].onfinish;
    drawers.toggleInlineDrawer(root, 160);
    const staleCloseFinish = animations[1].onfinish;
    expect(drawers.isInlineDrawerOpen(root)).toBe(false);
    expect(content.style.display).toBe('block');
    drawers.setInlineDrawerOpen(root, true);
    staleOpenFinish();
    staleCloseFinish();
    await Promise.resolve();
    expect(content.style.display).toBe('block');
    expect(completed).toEqual([]);

    drawers.toggleInlineDrawer(root, 0);
    drawers.setInlineDrawerOpen(root, true);
    await Promise.resolve();
    expect(completed).toEqual([]);
    drawers.toggleInlineDrawer(root, 160);
    animations.at(-1).finish();
    await Promise.resolve();
    expect(content.style.display).toBe('none');
    expect(completed).toEqual([false]);
});

test('external direction classes project without replaying notifications or fighting extension display', async () => {
    await render(inlineMarkup);
    const root = document.querySelector('.inline-drawer');
    const content = root.querySelector('.inline-drawer-content');
    const icon = root.querySelector('.inline-drawer-icon');
    const animations = controlMotion(content);
    const lazyStates = [];
    let completed = 0;
    root.addEventListener('inline-drawer-toggle', () => lazyStates.push(drawers.isInlineDrawerOpen(root)));
    root.addEventListener('inline-drawer-motion-complete', () => completed++);
    icon.classList.replace('down', 'up');
    root.dispatchEvent(new CustomEvent('inline-drawer-toggle', { bubbles: true }));
    expect(lazyStates).toEqual([true]);
    expect(content.style.display).toBe('none');
    content.style.display = 'flex';
    await nextTask();
    expect(icon.getAttribute('aria-expanded')).toBe('true');
    expect(lazyStates).toEqual([true]);

    drawers.toggleInlineDrawer(root, 160);
    const staleFinish = animations[0].onfinish;
    icon.classList.remove('down');
    icon.classList.add('up');
    content.style.display = 'flex';
    await nextTask();
    staleFinish();
    await Promise.resolve();
    expect(content.style.display).toBe('flex');
    expect(icon.getAttribute('aria-expanded')).toBe('true');
    expect(completed).toBe(0);
    expect(lazyStates).toEqual([true, false]);
});

test('cloned instances get their own associations and native or nested toggles receive projection', async () => {
    await render(`<div class="template_element">${inlineMarkup}</div>
        <div id="native" class="inline-drawer"><button type="button" class="inline-drawer-toggle" aria-label="Settings">
            <i class="inline-drawer-icon down"></i></button><div class="inline-drawer-content" style="display:none"></div></div>
        <div id="nested" class="inline-drawer"><div class="controls"><button class="maximize">Maximize</button>
            <div class="inline-drawer-toggle"><i class="inline-drawer-icon up"></i></div></div>
            <div class="inline-drawer-content" style="display:flex"></div></div>`);
    const template = document.querySelector('.template_element .inline-drawer');
    const first = template.cloneNode(true);
    const second = template.cloneNode(true);
    document.body.append(first, second);
    await nextTask();
    for (const root of [first, second]) {
        const icon = root.querySelector('.inline-drawer-icon');
        const content = root.querySelector('.inline-drawer-content');
        expect(document.getElementById(icon.getAttribute('aria-controls'))).toBe(content);
        expect(root.querySelector('.inline-drawer-header').hasAttribute('aria-expanded')).toBe(false);
    }
    const native = document.getElementById('native');
    drawers.setInlineDrawerOpen(native, true);
    expect(native.querySelector('button').getAttribute('aria-expanded')).toBe('true');
    expect(native.querySelector('i').hasAttribute('aria-expanded')).toBe(false);
    const nested = document.getElementById('nested');
    expect(nested.querySelector('.inline-drawer-icon').getAttribute('aria-expanded')).toBe('true');
    drawers.setInlineDrawerOpen(nested, false);
    expect(nested.querySelector('.inline-drawer-icon').getAttribute('aria-expanded')).toBe('false');
    expect(nested.querySelector('.maximize').hasAttribute('aria-expanded')).toBe(false);
});
