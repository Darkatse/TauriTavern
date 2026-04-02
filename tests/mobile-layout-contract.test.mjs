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

class HTMLIFrameElementMock extends HTMLElementMock {
    constructor() {
        super('iframe');
        this.contentDocument = null;
        this.contentWindow = null;
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
        createTreeWalker(rootNode, _whatToShow) {
            const nodes = [];
            const walk = (node) => {
                if (!(node instanceof HTMLElementMock)) {
                    return;
                }
                for (const child of node.children) {
                    nodes.push(child);
                    walk(child);
                }
            };
            walk(rootNode);

            let index = -1;
            return {
                currentNode: rootNode,
                nextNode() {
                    index += 1;
                    if (index >= nodes.length) {
                        return false;
                    }
                    this.currentNode = nodes[index];
                    return true;
                },
            };
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
    globalThis.NodeFilter = { SHOW_ELEMENT: 1 };

    globalThis.HTMLElement = HTMLElementMock;
    globalThis.HTMLBodyElement = HTMLBodyElementMock;
    globalThis.HTMLHeadElement = HTMLHeadElementMock;
    globalThis.HTMLIFrameElement = HTMLIFrameElementMock;
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
    assert.match(
        source,
        /\[data-tt-mobile-surface="viewport-host"\]\[data-tt-mobile-surface\]\[data-tt-mobile-surface\]/,
    );
});

test('geometry firewall implements fixed-shell IME contract (local keyboard offset)', async () => {
    const firewallPath = path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-geometry-firewall.js');
    const source = await readFile(firewallPath, 'utf8');

    assert.match(source, /\[data-tt-ime-surface="fixed-shell"\]\[data-tt-ime-active\]/);
    assert.match(source, /--tt-keyboard-offset/);
    assert.match(source, /\bscroll-padding-bottom\b/);
});

test('geometry firewall enforces viewport root contract (stable size + no root transform)', async () => {
    const firewallPath = path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-geometry-firewall.js');
    const source = await readFile(firewallPath, 'utf8');

    assert.match(source, /html,\s*[\r\n]+\s*body\s*\{\s*[\s\S]*height:\s*var\(--tt-base-viewport-height/);
    assert.match(source, /html\s*\{\s*[\s\S]*-webkit-transform:\s*none/);
    assert.match(source, /html\s*\{\s*[\s\S]*transform:\s*none/);
    assert.match(source, /html\s*\{\s*[\s\S]*perspective:\s*none/);
    assert.match(source, /html\s*\{\s*[\s\S]*backface-visibility:\s*hidden/);
});

test('geometry firewall defines viewport-host outer geometry contract (explicit size)', async () => {
    const firewallPath = path.join(REPO_ROOT, 'src/tauri/main/compat/mobile/mobile-geometry-firewall.js');
    const source = await readFile(firewallPath, 'utf8');

    assert.match(source, /\[data-tt-mobile-surface="viewport-host"\][\s\S]*width:\s*100vw/);
    assert.match(source, /\[data-tt-mobile-surface="viewport-host"\][\s\S]*width:\s*100dvw/);
    assert.match(source, /\[data-tt-mobile-surface="viewport-host"\][\s\S]*height:\s*var\(--tt-base-viewport-height/);
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
    assert.match(source, /\binstallMobileImeSurfaceController\b/);
    assert.match(source, /\binstallMobileOverlayCompatController\b/);
    assert.match(source, /\binstallMobileIframeViewportContractBridge\b/);
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

test('overlay classifier admits late-styled fixed overlays within bounded settle window', async () => {
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

    let frame = 0;
    dom.windowMock.requestAnimationFrame = (handler) => {
        frame += 1;
        handler();
        return 0;
    };
    globalThis.requestAnimationFrame = dom.windowMock.requestAnimationFrame;

    const surface = new HTMLElementMock('div');
    surface.setBoundingClientRect({
        top: 0,
        left: 0,
        right: 200,
        bottom: 200,
        width: 200,
        height: 200,
    });
    dom.body.appendChild(surface);

    const surfaceStyle = {
        position: 'fixed',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
        cursor: 'auto',
        touchAction: 'auto',
        pointerEvents: 'auto',
    };
    Object.defineProperty(surfaceStyle, 'top', {
        get() {
            if (frame === 0) return 'auto';
            if (frame === 1) return '0px';
            return '48px';
        },
    });
    dom.setComputedStyle(surface, surfaceStyle);

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(surface.getAttribute('data-tt-mobile-surface'), 'edge-window');
    assert.equal(surface.getAttribute('data-tt-mobile-surface-admitted'), '1');
    assert.equal(surface.style.getPropertyValue('--tt-original-top'), '48px');

    controller.dispose();
});

test('overlay classifier admits draggable fixed widgets as free-window (no --tt-original-top + admission nudge)', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '44px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-viewport-bottom-inset') return '0px';
            if (name === '--tt-inset-bottom') return '0px';
            return '';
        },
    });

    const widget = new HTMLElementMock('div');
    widget.className = 'fab';
    widget.setBoundingClientRect({
        top: 0,
        left: 0,
        right: 48,
        bottom: 48,
        width: 48,
        height: 48,
    });
    dom.body.appendChild(widget);

    dom.setComputedStyle(widget, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
        cursor: 'grab',
        touchAction: 'none',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(widget.getAttribute('data-tt-mobile-surface'), 'free-window');
    assert.equal(widget.style.getPropertyValue('--tt-original-top'), '');
    assert.equal(widget.style.getPropertyValue('top'), '44px');

    controller.dispose();
});

test('overlay classifier infers free-window via descendant drag affordance', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '44px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-viewport-bottom-inset') return '0px';
            if (name === '--tt-inset-bottom') return '0px';
            return '';
        },
    });

    const widget = new HTMLElementMock('div');
    widget.className = 'fab';
    widget.setBoundingClientRect({
        top: 0,
        left: 0,
        right: 48,
        bottom: 48,
        width: 48,
        height: 48,
    });

    const handle = new HTMLElementMock('button');
    widget.appendChild(handle);
    dom.body.appendChild(widget);

    dom.setComputedStyle(widget, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: 'auto',
        bottom: 'auto',
        cursor: 'auto',
        touchAction: 'auto',
    });

    dom.setComputedStyle(handle, {
        cursor: 'grab',
        touchAction: 'none',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(widget.getAttribute('data-tt-mobile-surface'), 'free-window');
    assert.equal(widget.style.getPropertyValue('--tt-original-top'), '');
    assert.equal(widget.style.getPropertyValue('top'), '44px');

    controller.dispose();
});

test('overlay classifier admits actual surfaces inside portal hosts (host is not treated as surface)', async () => {
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

    const portalHost = new HTMLElementMock('div');
    portalHost.setAttribute('script_id', 'portal-host');
    portalHost.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });

    const panel = new HTMLElementMock('div');
    panel.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });
    portalHost.appendChild(panel);
    dom.body.appendChild(portalHost);

    dom.setComputedStyle(portalHost, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: '0px',
        bottom: '0px',
        pointerEvents: 'none',
        cursor: 'auto',
        touchAction: 'auto',
    });

    dom.setComputedStyle(panel, {
        position: 'fixed',
        top: '0px',
        left: '0px',
        right: '0px',
        bottom: '0px',
        pointerEvents: 'auto',
        cursor: 'auto',
        touchAction: 'auto',
    });

    const overlayModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-overlay-compat-controller.js',
    );
    const { installMobileOverlayCompatController } = await import(pathToFileURL(overlayModulePath).href);

    const controller = installMobileOverlayCompatController();
    assert.equal(portalHost.getAttribute('data-tt-mobile-surface'), null);
    assert.equal(panel.getAttribute('data-tt-mobile-surface'), 'fullscreen-window');
    assert.equal(panel.getAttribute('data-tt-mobile-surface-admitted'), '1');

    controller.dispose();
});

test('overlay classifier admits same-origin iframe hosts as viewport-host (no --tt-original-top)', async () => {
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

    const iframe = new HTMLIFrameElementMock();
    iframe.setAttribute('script_id', 'test-script');
    iframe.setBoundingClientRect({
        top: 0,
        left: 0,
        right: dom.windowMock.innerWidth,
        bottom: dom.windowMock.innerHeight,
        width: dom.windowMock.innerWidth,
        height: dom.windowMock.innerHeight,
    });
    dom.body.appendChild(iframe);

    dom.setComputedStyle(iframe, {
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
    assert.equal(iframe.getAttribute('data-tt-mobile-surface'), 'viewport-host');
    assert.equal(iframe.style.getPropertyValue('--tt-original-top'), '');

    controller.revalidate();
    assert.equal(iframe.getAttribute('data-tt-mobile-surface'), 'viewport-host');
    assert.equal(iframe.style.getPropertyValue('--tt-original-top'), '');

    controller.dispose();
});

test('iframe viewport contract bridge syncs inset vars into same-origin iframe documents', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '44px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-bottom') return '34px';
            if (name === '--tt-viewport-bottom-inset') return '34px';
            if (name === '--tt-base-viewport-height') return '844px';
            return '';
        },
    });

    const iframe = new HTMLIFrameElementMock();
    const iframeRoot = new HTMLElementMock('html');
    iframe.contentDocument = { documentElement: iframeRoot };
    dom.body.appendChild(iframe);

    const bridgeModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-iframe-viewport-contract-bridge.js',
    );
    const { installMobileIframeViewportContractBridge } = await import(pathToFileURL(bridgeModulePath).href);

    const controller = installMobileIframeViewportContractBridge();
    controller.watchIframe(iframe);

    assert.equal(iframeRoot.style.getPropertyValue('--tt-inset-top'), '44px');
    assert.equal(iframeRoot.style.getPropertyValue('--tt-inset-right'), '0px');
    assert.equal(iframeRoot.style.getPropertyValue('--tt-inset-left'), '0px');
    assert.equal(iframeRoot.style.getPropertyValue('--tt-inset-bottom'), '34px');
    assert.equal(iframeRoot.style.getPropertyValue('--tt-viewport-bottom-inset'), '34px');
    assert.equal(iframeRoot.style.getPropertyValue('--tt-base-viewport-height'), '844px');

    controller.dispose();
});

test('iframe viewport contract bridge ignores cross-origin access failures', async () => {
    const dom = createDomHarness();
    dom.reset();

    dom.setComputedStyle(dom.documentElement, {
        getPropertyValue(name) {
            if (name === '--tt-inset-top') return '44px';
            if (name === '--tt-inset-right') return '0px';
            if (name === '--tt-inset-left') return '0px';
            if (name === '--tt-inset-bottom') return '34px';
            if (name === '--tt-viewport-bottom-inset') return '34px';
            if (name === '--tt-base-viewport-height') return '844px';
            return '';
        },
    });

    const iframe = new HTMLIFrameElementMock();
    Object.defineProperty(iframe, 'contentDocument', {
        get() {
            throw new Error('Cross origin');
        },
    });
    dom.body.appendChild(iframe);

    const bridgeModulePath = path.join(
        REPO_ROOT,
        'src/tauri/main/compat/mobile/mobile-iframe-viewport-contract-bridge.js',
    );
    const { installMobileIframeViewportContractBridge } = await import(pathToFileURL(bridgeModulePath).href);

    const controller = installMobileIframeViewportContractBridge();
    assert.doesNotThrow(() => controller.watchIframe(iframe));

    controller.dispose();
});
