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
        this.#values.set(name, String(value));
    }

    removeProperty(name) {
        this.#values.delete(name);
    }
}

class HTMLElementMock {
    constructor(tagName = 'div') {
        this.tagName = String(tagName).toUpperCase();
        /** @type {string} */
        this.id = '';
        /** @type {string} */
        this.className = '';
        /** @type {CssStyleDeclarationMock} */
        this.style = new CssStyleDeclarationMock();
        /** @type {HTMLElementMock | null} */
        this.parentElement = null;
        /** @type {HTMLElementMock[]} */
        this.children = [];
        /** @type {boolean} */
        this.isConnected = true;

        this.#attrs = new Map();
        this.#rect = { top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 };
    }

    /** @type {Map<string, string>} */
    #attrs;
    /** @type {{ top: number, left: number, right: number, bottom: number, width: number, height: number }} */
    #rect;

    getBoundingClientRect() {
        return { ...this.#rect };
    }

    setBoundingClientRect(rect) {
        this.#rect = { ...rect };
    }

    setAttribute(name, value) {
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
        child.isConnected = true;
        this.children.push(child);
        return child;
    }

    remove() {
        if (this.parentElement) {
            this.parentElement.children = this.parentElement.children.filter((node) => node !== this);
            this.parentElement = null;
        }
        this.isConnected = false;
    }

    closest(selector) {
        const ids = String(selector)
            .split(',')
            .map((part) => part.trim())
            .filter(Boolean)
            .map((part) => (part.startsWith('#') ? part.slice(1) : part));

        /** @type {HTMLElementMock | null} */
        let cursor = this;
        while (cursor) {
            if (cursor.id && ids.includes(cursor.id)) {
                return cursor;
            }
            cursor = cursor.parentElement;
        }
        return null;
    }

    querySelectorAll(selector) {
        if (String(selector).trim() !== '*') {
            return [];
        }

        /** @type {HTMLElementMock[]} */
        const result = [];

        const walk = (node) => {
            for (const child of node.children) {
                result.push(child);
                walk(child);
            }
        };

        walk(this);
        return result;
    }
}

class HTMLBodyElementMock extends HTMLElementMock {
    constructor() {
        super('body');
    }
}

class HTMLHeadElementMock extends HTMLElementMock {
    constructor() {
        super('head');
    }

    get lastElementChild() {
        return this.children.length > 0 ? this.children[this.children.length - 1] : null;
    }
}

class HTMLStyleElementMock extends HTMLElementMock {
    constructor() {
        super('style');
        this.type = '';
        this.textContent = '';
    }
}

class MutationObserverMock {
    constructor(callback) {
        this._callback = callback;
    }

    observe(_target, _options) {}
    disconnect() {}
}

function createDomHarness() {
    const documentElement = new HTMLElementMock('html');
    const head = new HTMLHeadElementMock();
    const body = new HTMLBodyElementMock();

    /** @type {WeakMap<any, any>} */
    const computedStyles = new WeakMap();

    const documentMock = {
        documentElement,
        head,
        body,
        getElementById(id) {
            const search = (node) => {
                if (node.id === id) {
                    return node;
                }
                for (const child of node.children) {
                    const found = search(child);
                    if (found) {
                        return found;
                    }
                }
                return null;
            };

            return search(head) || search(body);
        },
        createElement(tagName) {
            if (String(tagName).toLowerCase() === 'style') {
                return new HTMLStyleElementMock();
            }
            return new HTMLElementMock(tagName);
        },
        addEventListener(_type, _handler, _options) {},
    };

    const visualViewport = {
        width: 390,
        height: 844,
        addEventListener(_type, _handler, _options) {},
        removeEventListener(_type, _handler) {},
    };

    const windowMock = {
        innerWidth: 390,
        innerHeight: 844,
        visualViewport,
        addEventListener(_type, _handler, _options) {},
        removeEventListener(_type, _handler) {},
        requestAnimationFrame(handler) {
            handler();
            return 0;
        },
    };

    globalThis.window = windowMock;
    globalThis.document = documentMock;
    globalThis.MutationObserver = MutationObserverMock;
    globalThis.requestAnimationFrame = windowMock.requestAnimationFrame;

    globalThis.HTMLElement = HTMLElementMock;
    globalThis.HTMLBodyElement = HTMLBodyElementMock;
    globalThis.HTMLHeadElement = HTMLHeadElementMock;
    globalThis.HTMLStyleElement = HTMLStyleElementMock;

    globalThis.getComputedStyle = (target) => {
        const style = computedStyles.get(target);
        if (!style) {
            throw new Error('Missing computed style for target');
        }
        return style;
    };

    const setComputedStyle = (target, style) => {
        computedStyles.set(target, style);
    };

    const reset = () => {
        documentMock.head.children = [];
        documentMock.body.children = [];
        delete windowMock.__TAURITAVERN_MOBILE_OVERLAY_COMPAT__;
    };

    return { documentMock, documentElement, head, body, windowMock, setComputedStyle, reset };
}

test('geometry firewall surface selectors keep high specificity (>= Vue scoped)', async () => {
    const firewallPath = path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-geometry-firewall.js');
    const source = await readFile(firewallPath, 'utf8');

    assert.match(
        source,
        /\[data-tt-mobile-surface="fullscreen-window"\]\[data-tt-mobile-surface\]\[data-tt-mobile-surface\]/,
    );
    assert.match(source, /\[data-tt-mobile-surface="edge-window"\]\[data-tt-mobile-surface\]\[data-tt-mobile-surface\]/);
});

test('geometry firewall stays last in <head> (keep-last)', async () => {
    const dom = createDomHarness();
    dom.reset();

    const firewallModulePath = path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-geometry-firewall.js');
    const { installMobileGeometryFirewall } = await import(pathToFileURL(firewallModulePath).href);

    const controller = installMobileGeometryFirewall();
    assert.equal(dom.head.lastElementChild?.id, 'tt-mobile-geometry-firewall');

    const injected = new HTMLStyleElementMock();
    injected.id = 'third-party-style';
    dom.head.appendChild(injected);
    assert.equal(dom.head.lastElementChild?.id, 'third-party-style');

    controller.ensureLast();
    assert.equal(dom.head.lastElementChild?.id, 'tt-mobile-geometry-firewall');

    controller.dispose();
});

test('bootstrap wires mobile geometry firewall + overlay classifier (no old controller)', async () => {
    const bootstrapPath = path.join(REPO_ROOT, 'src/tauri/main/bootstrap.js');
    const source = await readFile(bootstrapPath, 'utf8');

    assert.match(source, /\binstallMobileGeometryFirewall\b/);
    assert.match(source, /\binstallMobileOverlayCompatController\b/);
    assert.doesNotMatch(source, /mobile-top-settings-layout-controller/);
});

test('overlay surface classifier is stable across revalidate (fullscreen-window)', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '0px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-viewport-bottom-inset') return '0px';
            if (name === '--tt-inset-bottom') return '0px';
            return '';
        },
    });

    const surface = new HTMLElementMock('div');
    surface.className = 'panel mobile';
    surface.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });
    dom.body.appendChild(surface);

    dom.setComputedStyle(surface, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'fullscreen-window');

    controller.revalidate();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'fullscreen-window');

    controller.dispose();
});

test('overlay classifier keeps fullscreen-window classification after safe-area insets apply', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '44px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-viewport-bottom-inset') return '34px';
            if (name === '--tt-inset-bottom') return '34px';
            return '';
        },
    });

    const viewportWidth = dom.windowMock.innerWidth;
    const viewportHeight = dom.windowMock.innerHeight;

    const surface = new HTMLElementMock('div');
    surface.className = 'acu-window maximized';
    surface.setBoundingClientRect({
        top: 44,
        left: 0,
        right: viewportWidth,
        bottom: viewportHeight - 34,
        width: viewportWidth,
        height: viewportHeight - 44 - 34,
    });
    dom.body.appendChild(surface);

    dom.setComputedStyle(surface, {
        position: 'fixed',
        top: '44px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'fullscreen-window');

    controller.revalidate();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'fullscreen-window');

    controller.dispose();
});

test('overlay classifier: backdrop detection via zero inset edges', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(_name) {
            return '0px';
        },
    });

    const overlay = new HTMLElementMock('div');
    overlay.className = 'random-mask';
    overlay.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });
    dom.body.appendChild(overlay);

    dom.setComputedStyle(overlay, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: '0px',
        bottom: '0px',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(overlay.getAttribute('data-tt-mobile-surface'), 'backdrop');

    controller.dispose();
});

test('overlay classifier respects explicit opt-in (does not override / does not write --tt-original-top)', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(_name) {
            return '0px';
        },
    });

    const surface = new HTMLElementMock('div');
    surface.className = 'panel mobile';
    surface.setAttribute('data-tt-mobile-surface', 'none');
    surface.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });
    dom.body.appendChild(surface);

    dom.setComputedStyle(surface, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'none');
    assert.equal(surface.style.getPropertyValue('--tt-original-top'), '');

    controller.dispose();
});

test('overlay classifier writes --tt-original-top for edge-window surfaces', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(_name) {
            return '0px';
        },
    });

    const surface = new HTMLElementMock('div');
    surface.className = 'toast';
    surface.setBoundingClientRect({
        top: 10,
        left: 0,
        right: 200,
        bottom: 110,
        width: 200,
        height: 100,
    });
    dom.body.appendChild(surface);

    dom.setComputedStyle(surface, {
        position: 'fixed',
        top: '10px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'edge-window');
    assert.equal(surface.style.getPropertyValue('--tt-original-top'), '10px');

    controller.dispose();
});
