import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

class CssStyleDeclarationMock {
    #values = new Map();

    getPropertyValue(name) {
        return this.#values.get(name) ?? '';
    }

    setProperty(name, value) {
        this.#values.set(String(name), String(value));
    }

    removeProperty(name) {
        this.#values.delete(String(name));
    }
}

class ElementMock {
    constructor(tagName = 'div') {
        this.tagName = String(tagName).toUpperCase();
        this.style = new CssStyleDeclarationMock();
        this.children = [];
        this.parentElement = null;
        this.#attrs = new Map();
    }

    /** @type {Map<string, string>} */
    #attrs;

    setAttribute(name, value) {
        this.#attrs.set(String(name), String(value));
    }

    getAttribute(name) {
        return this.#attrs.get(String(name)) ?? null;
    }

    hasAttribute(name) {
        return this.#attrs.has(String(name));
    }

    appendChild(child) {
        child.parentElement = this;
        this.children.push(child);
        return child;
    }
}

class HTMLElementMock extends ElementMock {}

class MutationObserverMock {
    constructor(_callback) {}
    observe(_target, _options) {}
    disconnect() {}
}

function createHarness() {
    globalThis.Element = ElementMock;
    globalThis.HTMLElement = HTMLElementMock;
    globalThis.MutationObserver = MutationObserverMock;
    globalThis.getComputedStyle = (element) => element.style;

    const documentElement = new HTMLElementMock('html');
    const body = new HTMLElementMock('body');
    const head = new HTMLElementMock('head');

    const documentMock = {
        documentElement,
        body,
        head,
        addEventListener(_type, _handler, _options) {},
        removeEventListener(_type, _handler, _options) {},
        querySelector(selector) {
            if (String(selector).trim() !== '[data-tt-ime-active]') {
                return null;
            }

            /** @type {ElementMock[]} */
            const queue = [documentElement, body, head];
            while (queue.length) {
                const node = queue.shift();
                if (!node) {
                    continue;
                }
                if (node.hasAttribute?.('data-tt-ime-active')) {
                    return node;
                }
                for (const child of node.children ?? []) {
                    queue.push(child);
                }
            }

            return null;
        },
    };

    const visualViewport = {
        width: 390,
        height: 844,
        offsetLeft: 0,
        offsetTop: 0,
        addEventListener(_type, _handler, _options) {},
        removeEventListener(_type, _handler, _options) {},
    };

    const windowMock = {
        innerWidth: 390,
        innerHeight: 844,
        visualViewport,
        addEventListener(_type, _handler, _options) {},
        removeEventListener(_type, _handler, _options) {},
        requestAnimationFrame(handler) {
            handler();
            return 0;
        },
        __TAURITAVERN__: { api: {} },
    };

    globalThis.document = documentMock;
    globalThis.window = windowMock;

    return { windowMock, documentMock };
}

test('api.layout installs and returns snapshots (safe-area + IME)', async () => {
    const { windowMock, documentMock } = createHarness();

    documentMock.documentElement.style.setProperty('--tt-inset-top', '10px');
    documentMock.documentElement.style.setProperty('--tt-inset-right', '4px');
    documentMock.documentElement.style.setProperty('--tt-inset-bottom', '20px');
    documentMock.documentElement.style.setProperty('--tt-inset-left', '6px');

    const activeSurface = new HTMLElementMock('div');
    activeSurface.setAttribute('data-tt-ime-active', '');
    activeSurface.setAttribute('data-tt-ime-surface', 'fixed-shell');
    activeSurface.style.setProperty('--tt-ime-bottom', '300px');
    documentMock.body.appendChild(activeSurface);

    const { installLayoutApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/layout.js')));
    installLayoutApi({});

    assert.ok(windowMock.__TAURITAVERN__?.api?.layout);
    const layout = windowMock.__TAURITAVERN__.api.layout;

    const snap = layout.snapshot();
    assert.equal(snap.safeInsets.top, 10);
    assert.equal(snap.safeInsets.right, 4);
    assert.equal(snap.safeInsets.bottom, 20);
    assert.equal(snap.safeInsets.left, 6);

    assert.equal(snap.viewport.width, 390);
    assert.equal(snap.viewport.height, 844);

    assert.equal(snap.safeFrame.top, 10);
    assert.equal(snap.safeFrame.left, 6);
    assert.equal(snap.safeFrame.width, 390 - 6 - 4);
    assert.equal(snap.safeFrame.height, 844 - 10 - 20);

    assert.equal(snap.ime.activeSurface, activeSurface);
    assert.equal(snap.ime.kind, 'fixed-shell');
    assert.equal(snap.ime.bottom, 300);
    assert.equal(snap.ime.viewportBottomInset, 300);
    assert.equal(snap.ime.keyboardOffset, 280);

    let calls = 0;
    const unsubscribe = await layout.subscribe((next) => {
        calls += 1;
        assert.equal(next.safeInsets.top, 10);
    });
    assert.equal(calls, 1);
    await unsubscribe();
    await unsubscribe();
});

test('layout-kit.js exists and exposes stable surface strings', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/tauritavern/layout-kit.js'), 'utf8');
    assert.match(source, /export const SURFACE/);
    assert.match(source, /data-tt-mobile-surface/);
    assert.match(source, /fullscreen-window/);
    assert.match(source, /viewport-host/);
});

