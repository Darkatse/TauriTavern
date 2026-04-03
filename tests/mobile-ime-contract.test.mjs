import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

class HTMLElementMock {
    constructor(tagName = 'div') {
        this.tagName = String(tagName).toUpperCase();
        this.id = '';
        this.className = '';
        this.parentElement = null;
        this.children = [];

        this.#attrs = new Map();
    }

    /** @type {Map<string, string>} */
    #attrs;

    setAttribute(name, value = '') {
        this.#attrs.set(String(name), String(value));
    }

    getAttribute(name) {
        return this.#attrs.get(String(name)) ?? null;
    }

    hasAttribute(name) {
        return this.#attrs.has(String(name));
    }

    removeAttribute(name) {
        this.#attrs.delete(String(name));
    }

    appendChild(child) {
        if (!(child instanceof HTMLElementMock)) {
            throw new Error('appendChild expects an HTMLElementMock');
        }
        if (child.parentElement) {
            child.parentElement.children = child.parentElement.children.filter((node) => node !== child);
        }
        child.parentElement = this;
        this.children.push(child);
        return child;
    }

    contains(target) {
        if (!(target instanceof HTMLElementMock)) {
            return false;
        }
        if (target === this) {
            return true;
        }
        for (const child of this.children) {
            if (child.contains(target)) {
                return true;
            }
        }
        return false;
    }

    closest(selector) {
        const selectors = String(selector)
            .split(',')
            .map((part) => part.trim())
            .filter(Boolean);

        /** @type {HTMLElementMock | null} */
        let cursor = this;
        while (cursor) {
            for (const rule of selectors) {
                if (matchesSelector(cursor, rule)) {
                    return cursor;
                }
            }
            cursor = cursor.parentElement;
        }
        return null;
    }
}

class HTMLTextAreaElementMock extends HTMLElementMock {
    constructor() {
        super('textarea');
    }
}

class HTMLInputElementMock extends HTMLElementMock {
    constructor() {
        super('input');
    }
}

function matchesSelector(element, selector) {
    if (!selector) {
        return false;
    }

    if (selector.startsWith('#')) {
        return element.id === selector.slice(1);
    }

    if (selector.startsWith('[') && selector.endsWith(']')) {
        const attrName = selector.slice(1, -1).trim();
        return element.hasAttribute(attrName);
    }

    if (selector === 'dialog.popup[open]') {
        if (element.tagName !== 'DIALOG') {
            return false;
        }
        if (!String(element.className).split(/\s+/).includes('popup')) {
            return false;
        }
        return element.hasAttribute('open');
    }

    if (selector.startsWith('.')) {
        const required = selector
            .split('.')
            .map((part) => part.trim())
            .filter(Boolean);
        const classes = String(element.className).split(/\s+/).filter(Boolean);
        return required.every((name) => classes.includes(name));
    }

    return false;
}

function createFocusHarness({ android = true } = {}) {
    /** @type {Map<string, { handler: any, capture: boolean }[]>} */
    const listeners = new Map();

    const head = new HTMLElementMock('head');
    const body = new HTMLElementMock('body');

    const documentMock = {
        head,
        body,
        activeElement: null,
        getElementById(id) {
            const needle = String(id);
            const walk = (node) => {
                if (node.id === needle) {
                    return node;
                }
                for (const child of node.children) {
                    const found = walk(child);
                    if (found) {
                        return found;
                    }
                }
                return null;
            };
            return walk(head) || walk(body);
        },
        addEventListener(type, handler, capture) {
            const key = String(type);
            const bucket = listeners.get(key) ?? [];
            bucket.push({ handler, capture: Boolean(capture) });
            listeners.set(key, bucket);
        },
        removeEventListener(type, handler, capture) {
            const key = String(type);
            const bucket = listeners.get(key) ?? [];
            const next = bucket.filter(
                (item) => item.handler !== handler || item.capture !== Boolean(capture),
            );
            if (next.length === 0) {
                listeners.delete(key);
            } else {
                listeners.set(key, next);
            }
        },
    };

    const calls = [];
    const windowMock = {
        __TAURITAVERN_INSETS__: {
            setImeTarget(target) {
                calls.push(target);
            },
        },
    };

    globalThis.window = windowMock;
    globalThis.document = documentMock;

    globalThis.HTMLElement = HTMLElementMock;
    globalThis.HTMLTextAreaElement = HTMLTextAreaElementMock;
    globalThis.HTMLInputElement = HTMLInputElementMock;

    Object.defineProperty(globalThis, 'navigator', {
        value: android
            ? {
                userAgent: 'Mozilla/5.0 (Linux; Android 14) TauriTavern',
                maxTouchPoints: 5,
                platform: 'Android',
            }
            : {
                userAgent: 'Mozilla/5.0 (X11; Linux x86_64)',
                maxTouchPoints: 0,
                platform: 'Linux',
            },
        configurable: true,
    });

    const emit = (type, event) => {
        const bucket = listeners.get(String(type)) ?? [];
        for (const item of bucket) {
            item.handler(event);
        }
    };

    return { head, body, documentMock, windowMock, listeners, calls, emit };
}

test('Android insets bridge readiness gate: about:blank + #sheld', async () => {
    const bridgePath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidInsetsBridge.kt',
    );
    const source = await readFile(bridgePath, 'utf8');

    assert.match(source, /location\.href !== 'about:blank'/);
    assert.match(source, /document\.getElementById\('sheld'\)/);
});

test('Android insets helper exposes routing APIs and clears old IME target residue', async () => {
    const helperPath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/WebViewInsetsStyleApplier.kt',
    );
    const source = await readFile(helperPath, 'utf8');

    assert.match(source, /window\.__TAURITAVERN_INSETS__\s*=\s*\{/);
    assert.match(source, /\bsetImeTarget\b/);
    assert.match(source, /\breapply\b/);
    assert.match(source, /removeProperty\(IME_BOTTOM_VAR\)/);
    assert.match(source, /applyImeBottom\(state\.lastImeBottomCss\)/);
    assert.match(source, /document\.getElementById\('sheld'\)/);
});

test('Android insets injection resets on main-frame navigation', async () => {
    const clientPath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/RustWebViewClient.kt',
    );
    const clientSource = await readFile(clientPath, 'utf8');

    assert.match(clientSource, /\bmainFrameNavigationListener\b/);
    assert.match(clientSource, /onMainFramePageStarted/);
    assert.match(clientSource, /mainFrameNavigationListener\?\.\s*onMainFramePageStarted\(/);

    const activityPath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt',
    );
    const activitySource = await readFile(activityPath, 'utf8');

    assert.match(activitySource, /installWebViewNavigationHooks/);
    assert.match(activitySource, /RustWebViewClient\.mainFrameNavigationListener\s*=/);
    assert.match(activitySource, /insetsBridge\.onMainFrameNavigationStarted\(\)/);
    assert.match(activitySource, /RustWebViewClient\.mainFrameNavigationListener\s*=\s*null/);

    const bridgePath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidInsetsBridge.kt',
    );
    const bridgeSource = await readFile(bridgePath, 'utf8');

    assert.match(bridgeSource, /fun onMainFrameNavigationStarted\(\)/);
    assert.match(bridgeSource, /hasReadyPageInsetsInjection\s*=\s*false/);
    assert.match(bridgeSource, /webViewInsetsStyleApplier\.onWebViewContextReset\(\)/);
});

test('mobile IME surface controller is focus-driven (no observers) and writes host-private attrs', async () => {
    const controllerPath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-ime-surface-controller.js',
    );
    const source = await readFile(controllerPath, 'utf8');

    assert.match(source, /\bdocument\.addEventListener\('focusin'/);
    assert.match(source, /\bdocument\.addEventListener\('focusout'/);
    assert.match(source, /data-tt-ime-surface/);
    assert.match(source, /data-tt-ime-active/);
    assert.match(source, /\bsetImeTarget\b/);
    assert.doesNotMatch(source, /\bMutationObserver\b/);
});

test('fixed-shell IME contract stays geometry-only (no pseudo-element spacer)', async () => {
    const firewallPath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-geometry-firewall.js',
    );
    const source = await readFile(firewallPath, 'utf8');

    assert.doesNotMatch(source, /data-tt-ime-surface="fixed-shell"[\s\S]*::after/);
});

test('focus routing: composer ↔ fixed-shell toggles active attrs + bridge target', async () => {
    const dom = createFocusHarness();

    const sheld = new HTMLElementMock('div');
    sheld.id = 'sheld';
    dom.body.appendChild(sheld);

    const sendTextarea = new HTMLTextAreaElementMock();
    sendTextarea.id = 'send_textarea';
    sheld.appendChild(sendTextarea);

    const characterPopup = new HTMLElementMock('div');
    characterPopup.id = 'character_popup';
    dom.body.appendChild(characterPopup);

    const editorTextarea = new HTMLTextAreaElementMock();
    characterPopup.appendChild(editorTextarea);

    const modulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-ime-surface-controller.js',
    );
    const { installMobileImeSurfaceController } = await importFresh(modulePath);

    const controller = installMobileImeSurfaceController();
    assert.ok(controller);

    dom.emit('focusin', { target: sendTextarea });
    assert.equal(sheld.getAttribute('data-tt-ime-surface'), 'composer');
    assert.ok(sheld.hasAttribute('data-tt-ime-active'));
    assert.equal(dom.calls.length, 0);

    dom.emit('focusin', { target: editorTextarea });
    assert.equal(characterPopup.getAttribute('data-tt-ime-surface'), 'fixed-shell');
    assert.ok(characterPopup.hasAttribute('data-tt-ime-active'));
    assert.equal(sheld.getAttribute('data-tt-ime-surface'), null);
    assert.equal(dom.calls.length, 1);
    assert.equal(dom.calls[0], characterPopup);

    dom.emit('focusin', { target: sendTextarea });
    assert.equal(sheld.getAttribute('data-tt-ime-surface'), 'composer');
    assert.ok(sheld.hasAttribute('data-tt-ime-active'));
    assert.equal(characterPopup.getAttribute('data-tt-ime-surface'), null);
    assert.equal(dom.calls.length, 2);
    assert.equal(dom.calls[1], null);

    controller.dispose();
});

test('focus routing: dialog surfaces are classified + routed, then restored on blur', async () => {
    const dom = createFocusHarness();

    const sheld = new HTMLElementMock('div');
    sheld.id = 'sheld';
    dom.body.appendChild(sheld);

    const dialog = new HTMLElementMock('dialog');
    dialog.className = 'popup';
    dialog.setAttribute('open', '');
    dom.body.appendChild(dialog);

    const dialogTextarea = new HTMLTextAreaElementMock();
    dialog.appendChild(dialogTextarea);

    const modulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-ime-surface-controller.js',
    );
    const { installMobileImeSurfaceController } = await importFresh(modulePath);

    const controller = installMobileImeSurfaceController();
    assert.ok(controller);

    dom.emit('focusin', { target: dialogTextarea });
    assert.equal(dialog.getAttribute('data-tt-ime-surface'), 'dialog');
    assert.ok(dialog.hasAttribute('data-tt-ime-active'));
    assert.equal(dom.calls.length, 1);
    assert.equal(dom.calls[0], dialog);

    dom.documentMock.activeElement = dom.body;
    dom.emit('focusout', { target: dialogTextarea });
    await Promise.resolve();

    assert.equal(dialog.getAttribute('data-tt-ime-surface'), null);
    assert.equal(dom.calls.length, 2);
    assert.equal(dom.calls[1], null);

    controller.dispose();
});

test('focus routing fails fast when bridge is missing (Android only)', async () => {
    const dom = createFocusHarness();
    delete dom.windowMock.__TAURITAVERN_INSETS__;

    const sheld = new HTMLElementMock('div');
    sheld.id = 'sheld';
    dom.body.appendChild(sheld);

    const characterPopup = new HTMLElementMock('div');
    characterPopup.id = 'character_popup';
    dom.body.appendChild(characterPopup);

    const editorTextarea = new HTMLTextAreaElementMock();
    characterPopup.appendChild(editorTextarea);

    const modulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-ime-surface-controller.js',
    );
    const { installMobileImeSurfaceController } = await importFresh(modulePath);

    const controller = installMobileImeSurfaceController();
    assert.ok(controller);

    assert.throws(
        () => dom.emit('focusin', { target: editorTextarea }),
        /\[TauriTavern\] Android insets bridge unavailable while routing IME target\./,
    );

    controller.dispose();
});

test('controller does not install on non-Android runtimes', async () => {
    const dom = createFocusHarness({ android: false });

    const modulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-ime-surface-controller.js',
    );
    const { installMobileImeSurfaceController } = await importFresh(modulePath);

    const controller = installMobileImeSurfaceController();
    assert.equal(controller, null);
    assert.equal(dom.listeners.size, 0);
});
