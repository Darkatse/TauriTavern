export function installFakeDom(options = {}) {
    const {
        innerWidth = 800,
        innerHeight = 600,
        userAgent = 'node',
        platform = 'node',
        maxTouchPoints = 0,
    } = options;

    const patchedGlobals = new Map();

    /**
     * @param {string} key
     * @param {any} value
     */
    const patchGlobal = (key, value) => {
        patchedGlobals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
        Object.defineProperty(globalThis, key, {
            value,
            writable: true,
            enumerable: true,
            configurable: true,
        });
    };

    let nowMs = 0;
    /** @type {Array<() => void>} */
    const microtasks = [];
    /** @type {Array<((ts: number) => void) | null>} */
    const rafs = [];

    const createdMutationObservers = [];
    const createdIntersectionObservers = [];

    class FakeNode {
        /** @type {FakeNode | null} */
        parentNode = null;
        /** @type {FakeNode[]} */
        childNodes = [];
        /** @type {boolean} */
        _connected = false;

        /** @param {FakeNode} node */
        appendChild(node) {
            if (node.parentNode) {
                node.parentNode.removeChild(node);
            }
            node.parentNode = this;
            this.childNodes.push(node);
            if (this._connected) {
                node._setConnected(true);
            }
            return node;
        }

        /** @param {FakeNode} node */
        removeChild(node) {
            const idx = this.childNodes.indexOf(node);
            if (idx >= 0) {
                this.childNodes.splice(idx, 1);
                node.parentNode = null;
                node._setConnected(false);
            }
            return node;
        }

        remove() {
            if (this.parentNode) {
                this.parentNode.removeChild(this);
            }
        }

        /** @param {boolean} connected */
        _setConnected(connected) {
            this._connected = connected;
            for (const child of this.childNodes) {
                child._setConnected(connected);
            }
        }

        get isConnected() {
            return this._connected;
        }
    }

    class FakeClassList {
        /** @param {Set<string>} set */
        constructor(set) {
            this._set = set;
        }

        /** @type {Set<string>} */
        _set;

        /** @param {...string} names */
        add(...names) {
            for (const name of names) {
                if (name) {
                    this._set.add(String(name));
                }
            }
        }

        /** @param {...string} names */
        remove(...names) {
            for (const name of names) {
                this._set.delete(String(name));
            }
        }

        /** @param {string} name */
        contains(name) {
            return this._set.has(String(name));
        }

        /** @param {string} name */
        toggle(name, force) {
            const key = String(name);
            if (typeof force === 'boolean') {
                if (force) {
                    this._set.add(key);
                    return true;
                }
                this._set.delete(key);
                return false;
            }

            if (this._set.has(key)) {
                this._set.delete(key);
                return false;
            }
            this._set.add(key);
            return true;
        }
    }

    function decodeHtmlEntities(text) {
        return String(text || '')
            .replace(/&lt;/g, '<')
            .replace(/&gt;/g, '>')
            .replace(/&amp;/g, '&');
    }

    function toDatasetKeyFromDataAttr(attrName) {
        const name = String(attrName).slice('data-'.length);
        return name.replace(/-([a-z])/g, (_m, c) => String(c).toUpperCase());
    }

    function parseAttributes(attrText) {
        /** @type {Map<string, string>} */
        const attrs = new Map();
        const text = String(attrText || '');
        const re = /([a-zA-Z_:][a-zA-Z0-9_:\-.]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g;
        for (;;) {
            const m = re.exec(text);
            if (!m) break;
            const name = m[1];
            const value = m[2] ?? m[3] ?? m[4] ?? '';
            attrs.set(name, value);
        }
        return attrs;
    }

    function parsePreBlocks(html) {
        const blocks = [];
        const re = /<pre\b([^>]*)>([\s\S]*?)<\/pre>/gi;
        for (;;) {
            const m = re.exec(String(html || ''));
            if (!m) break;
            const attrsText = m[1] || '';
            const content = m[2] || '';
            const attrs = parseAttributes(attrsText);
            const codeMatch = content.match(/<code\b[^>]*>([\s\S]*?)<\/code>/i);
            const raw = codeMatch ? codeMatch[1] : content;
            blocks.push({
                attrs,
                codeText: decodeHtmlEntities(raw),
            });
        }
        return blocks;
    }

    function parseSimpleSelector(selector) {
        const text = String(selector || '').trim();
        if (!text) {
            return null;
        }

        /** @type {{ tag: string | null; id: string | null; classes: string[]; attrs: Array<{ name: string; value: string | null }> }} */
        const parsed = { tag: null, id: null, classes: [], attrs: [] };

        let rest = text;
        const tagMatch = rest.match(/^[a-zA-Z][a-zA-Z0-9-]*/);
        if (tagMatch) {
            parsed.tag = tagMatch[0].toLowerCase();
            rest = rest.slice(tagMatch[0].length);
        }

        while (rest) {
            if (rest.startsWith('#')) {
                const m = rest.slice(1).match(/^[^\.\[#]+/);
                if (!m) break;
                parsed.id = m[0];
                rest = rest.slice(1 + m[0].length);
                continue;
            }
            if (rest.startsWith('.')) {
                const m = rest.slice(1).match(/^[^\.\[#]+/);
                if (!m) break;
                parsed.classes.push(m[0]);
                rest = rest.slice(1 + m[0].length);
                continue;
            }
            if (rest.startsWith('[')) {
                const end = rest.indexOf(']');
                if (end < 0) break;
                const body = rest.slice(1, end).trim();
                const eq = body.indexOf('=');
                if (eq < 0) {
                    parsed.attrs.push({ name: body, value: null });
                } else {
                    const name = body.slice(0, eq).trim();
                    let value = body.slice(eq + 1).trim();
                    value = value.replace(/^"(.+)"$/, '$1').replace(/^'(.+)'$/, '$1');
                    parsed.attrs.push({ name, value });
                }
                rest = rest.slice(end + 1);
                continue;
            }
            break;
        }

        return parsed;
    }

    function matchesSimple(el, selector) {
        const parsed = parseSimpleSelector(selector);
        if (!parsed) {
            return false;
        }
        if (parsed.tag && el.tagName.toLowerCase() !== parsed.tag) {
            return false;
        }
        if (parsed.id && el.id !== parsed.id) {
            return false;
        }
        for (const cls of parsed.classes) {
            if (!el.classList.contains(cls)) {
                return false;
            }
        }
        for (const attr of parsed.attrs) {
            const name = attr.name;
            if (name.startsWith('data-')) {
                const key = toDatasetKeyFromDataAttr(name);
                if (attr.value === null) {
                    if (!(key in el.dataset)) return false;
                } else if (String(el.dataset[key] ?? '') !== attr.value) {
                    return false;
                }
                continue;
            }

            if (attr.value === null) {
                if (el.getAttribute(name) === null) return false;
            } else if (String(el.getAttribute(name) ?? '') !== attr.value) {
                return false;
            }
        }
        return true;
    }

    function collectDescendants(root) {
        /** @type {FakeElement[]} */
        const out = [];

        /** @param {FakeNode} node */
        const walk = (node) => {
            for (const child of node.childNodes) {
                if (child instanceof FakeElement) {
                    out.push(child);
                }
                walk(child);
            }
        };
        walk(root);
        return out;
    }

    function querySelectorAllWithin(root, selector) {
        const parts = String(selector || '')
            .split(',')
            .map((s) => s.trim())
            .filter(Boolean);
        if (parts.length === 0) {
            return [];
        }

        /** @type {FakeElement[]} */
        const results = [];
        const seen = new Set();

        const allDesc = collectDescendants(root);

        for (const part of parts) {
            if (part.startsWith(':scope')) {
                const direct = part.replace(/^:scope\s*>\s*/i, '').trim();
                if (!direct) continue;
                for (const child of root.childNodes) {
                    if (child instanceof FakeElement && matchesSimple(child, direct)) {
                        if (!seen.has(child)) {
                            seen.add(child);
                            results.push(child);
                        }
                    }
                }
                continue;
            }

            const chain = part.split(/\s+/).filter(Boolean);
            if (chain.length === 1) {
                for (const el of allDesc) {
                    if (matchesSimple(el, chain[0])) {
                        if (!seen.has(el)) {
                            seen.add(el);
                            results.push(el);
                        }
                    }
                }
                continue;
            }

            const last = chain[chain.length - 1];
            for (const el of allDesc) {
                if (!matchesSimple(el, last)) {
                    continue;
                }
                let cursor = /** @type {FakeNode | null} */ (el.parentNode);
                let ok = true;
                for (let i = chain.length - 2; i >= 0; i -= 1) {
                    const need = chain[i];
                    let found = null;
                    while (cursor) {
                        if (cursor instanceof FakeElement && matchesSimple(cursor, need)) {
                            found = cursor;
                            cursor = cursor.parentNode;
                            break;
                        }
                        cursor = cursor.parentNode;
                    }
                    if (!found) {
                        ok = false;
                        break;
                    }
                }
                if (ok && !seen.has(el)) {
                    seen.add(el);
                    results.push(el);
                }
            }
        }

        return results;
    }

    class FakeElement extends FakeNode {
        /** @param {string} tagName */
        constructor(tagName) {
            super();
            this.tagName = String(tagName || '').toUpperCase();
        }

        /** @type {string} */
        tagName;

        /** @type {Map<string, string>} */
        _attrs = new Map();

        /** @type {Record<string, string>} */
        dataset = Object.create(null);

        /** @type {Set<string>} */
        _classes = new Set();

        classList = new FakeClassList(this._classes);

        /** @type {Record<string, string>} */
        style = Object.create(null);

        /** @type {string} */
        id = '';

        /** @type {number} */
        tabIndex = -1;

        get className() {
            return [...this._classes.values()].join(' ');
        }

        set className(value) {
            this.setAttribute('class', String(value ?? ''));
        }

        /** @type {Record<string, Array<Function>>} */
        _listeners = Object.create(null);

        /** @type {string | null} */
        _textContent = null;

        /** @type {{ top: number; right: number; bottom: number; left: number; width: number; height: number }} */
        _rect = { top: 0, right: 0, bottom: 0, left: 0, width: 0, height: 0 };

        get parentElement() {
            return this.parentNode instanceof FakeElement ? this.parentNode : null;
        }

        get children() {
            return this.childNodes.filter((n) => n instanceof FakeElement);
        }

        get previousElementSibling() {
            const parent = this.parentElement;
            if (!parent) return null;
            const siblings = parent.children;
            const idx = siblings.indexOf(this);
            return idx > 0 ? siblings[idx - 1] : null;
        }

        get nextElementSibling() {
            const parent = this.parentElement;
            if (!parent) return null;
            const siblings = parent.children;
            const idx = siblings.indexOf(this);
            return idx >= 0 && idx + 1 < siblings.length ? siblings[idx + 1] : null;
        }

        /** @param {...FakeNode} nodes */
        append(...nodes) {
            for (const node of nodes) {
                this.appendChild(node);
            }
        }

        /**
         * Minimal cloneNode implementation for contract tests.
         * @param {boolean} [deep]
         */
        cloneNode(deep = false) {
            const tag = String(this.tagName || '').toLowerCase();
            const doc = globalThis.document;
            const clone = doc && typeof doc.createElement === 'function'
                ? doc.createElement(tag)
                : new HTMLElement(tag);

            for (const [name, value] of this._attrs.entries()) {
                clone.setAttribute(name, value);
            }

            if (this._textContent !== null) {
                clone.textContent = this._textContent;
            }

            Object.assign(clone.style, this.style);

            clone._rect = { ...this._rect };

            if ('src' in this) {
                clone.src = this.src;
            }
            if ('srcdoc' in this) {
                clone.srcdoc = this.srcdoc;
            }
            if ('offsetHeight' in this) {
                clone.offsetHeight = this.offsetHeight;
            }

            if (deep) {
                for (const child of this.childNodes) {
                    if (child instanceof FakeElement) {
                        clone.appendChild(child.cloneNode(true));
                    }
                }
            }

            return clone;
        }

        /** @param {string} name @param {string} value */
        setAttribute(name, value) {
            const key = String(name);
            const val = String(value ?? '');
            this._attrs.set(key, val);
            if (key === 'id') {
                this.id = val;
            }
            if (key === 'class') {
                this._classes.clear();
                for (const cls of val.split(/\s+/).filter(Boolean)) {
                    this._classes.add(cls);
                }
            }
            if (key.startsWith('data-')) {
                const datasetKey = toDatasetKeyFromDataAttr(key);
                this.dataset[datasetKey] = val;
            }
        }

        /** @param {string} name */
        getAttribute(name) {
            const key = String(name);
            return this._attrs.has(key) ? this._attrs.get(key) : null;
        }

        /** @param {string} name */
        removeAttribute(name) {
            const key = String(name);
            this._attrs.delete(key);
            if (key === 'id') {
                this.id = '';
            }
            if (key === 'class') {
                this._classes.clear();
            }
            if (key.startsWith('data-')) {
                const datasetKey = toDatasetKeyFromDataAttr(key);
                delete this.dataset[datasetKey];
            }
        }

        /** @param {string} selector */
        matches(selector) {
            return matchesSimple(this, selector);
        }

        /** @param {string} selector */
        closest(selector) {
            let cursor = /** @type {FakeElement | null} */ (this);
            while (cursor) {
                if (cursor.matches(selector)) {
                    return cursor;
                }
                cursor = cursor.parentElement;
            }
            return null;
        }

        /** @param {FakeNode} node */
        contains(node) {
            let cursor = node;
            while (cursor) {
                if (cursor === this) return true;
                cursor = cursor.parentNode;
            }
            return false;
        }

        /** @param {string} selector */
        querySelector(selector) {
            const all = this.querySelectorAll(selector);
            return all.length > 0 ? all[0] : null;
        }

        /** @param {string} selector */
        querySelectorAll(selector) {
            return querySelectorAllWithin(this, selector);
        }

        addEventListener(type, listener) {
            const key = String(type);
            const list = this._listeners[key] || (this._listeners[key] = []);
            list.push(listener);
        }

        removeEventListener(type, listener) {
            const key = String(type);
            const list = this._listeners[key];
            if (!list) return;
            const idx = list.indexOf(listener);
            if (idx >= 0) list.splice(idx, 1);
        }

        dispatchEvent(event) {
            const key = String(event?.type || '');
            const list = (this._listeners[key] || []).slice();
            for (const fn of list) {
                fn.call(this, event);
            }
            return true;
        }

        set textContent(value) {
            this._textContent = value === null ? null : String(value);
        }

        get textContent() {
            if (this._textContent !== null) {
                return this._textContent;
            }
            let out = '';
            for (const child of this.childNodes) {
                if (child instanceof FakeElement) {
                    out += String(child.textContent || '');
                }
            }
            return out;
        }

        set innerHTML(html) {
            // Minimal parser: only materializes <pre><code>...</code></pre> blocks.
            this.childNodes.slice().forEach((n) => n.remove());
            for (const block of parsePreBlocks(html)) {
                const pre = new HTMLPreElement();
                for (const [name, value] of block.attrs.entries()) {
                    pre.setAttribute(name, value);
                }
                const code = new HTMLCodeElement();
                code.textContent = block.codeText;
                pre.append(code);
                this.append(pre);
            }
        }

        get innerHTML() {
            return '';
        }

        getBoundingClientRect() {
            return { ...this._rect };
        }

        /** @param {Partial<{ top: number; right: number; bottom: number; left: number; width: number; height: number }>} rect */
        _setRect(rect) {
            this._rect = { ...this._rect, ...rect };
        }

        /** @param {...FakeNode} nodes */
        before(...nodes) {
            const parent = this.parentNode;
            if (!(parent instanceof FakeNode)) return;
            const idx = parent.childNodes.indexOf(this);
            if (idx < 0) return;
            for (const node of nodes) {
                if (node.parentNode) {
                    node.parentNode.removeChild(node);
                }
                node.parentNode = parent;
                parent.childNodes.splice(idx, 0, node);
                if (parent._connected) {
                    node._setConnected(true);
                }
            }
        }

        /** @param {...FakeNode} nodes */
        replaceWith(...nodes) {
            const parent = this.parentNode;
            if (!(parent instanceof FakeNode)) return;
            const idx = parent.childNodes.indexOf(this);
            if (idx < 0) return;
            parent.childNodes.splice(idx, 1);
            this.parentNode = null;
            this._setConnected(false);
            for (let i = 0; i < nodes.length; i += 1) {
                const node = nodes[i];
                if (node.parentNode) {
                    node.parentNode.removeChild(node);
                }
                node.parentNode = parent;
                parent.childNodes.splice(idx + i, 0, node);
                if (parent._connected) {
                    node._setConnected(true);
                }
            }
        }
    }

    class HTMLElement extends FakeElement {
        constructor(tagName = 'div') {
            super(tagName);
        }
    }

    class HTMLDivElement extends HTMLElement {
        constructor() {
            super('div');
        }
    }

    class HTMLPreElement extends HTMLElement {
        constructor() {
            super('pre');
        }
    }

    class HTMLCodeElement extends HTMLElement {
        constructor() {
            super('code');
        }
    }

    class HTMLIFrameElement extends HTMLElement {
        constructor() {
            super('iframe');
        }

        /** @type {string} */
        src = '';

        /** @type {string} */
        srcdoc = '';

        /** @type {number} */
        offsetHeight = 0;
    }

    class DocumentFragment extends FakeNode {
        /** @param {string} selector */
        querySelectorAll(selector) {
            return querySelectorAllWithin(this, selector);
        }
    }

    class HTMLTemplateElement extends HTMLElement {
        constructor() {
            super('template');
            this.content = new DocumentFragment();
        }

        /** @type {DocumentFragment} */
        content;

        set innerHTML(html) {
            this.content.childNodes.slice().forEach((n) => n.remove());
            for (const block of parsePreBlocks(html)) {
                const pre = new HTMLPreElement();
                for (const [name, value] of block.attrs.entries()) {
                    pre.setAttribute(name, value);
                }
                const code = new HTMLCodeElement();
                code.textContent = block.codeText;
                pre.append(code);
                this.content.appendChild(pre);
            }
        }
    }

    class Document extends FakeNode {
        constructor() {
            super();
            this.head = new HTMLDivElement();
            this.head.tagName = 'HEAD';
            this.body = new HTMLDivElement();
            this.body.tagName = 'BODY';
            this.appendChild(this.head);
            this.appendChild(this.body);
            this._setConnected(true);
        }

        /** @type {FakeElement} */
        head;

        /** @type {FakeElement} */
        body;

        /** @param {string} tagName */
        createElement(tagName) {
            const tag = String(tagName || '').toLowerCase();
            if (tag === 'template') return new HTMLTemplateElement();
            if (tag === 'iframe') return new HTMLIFrameElement();
            if (tag === 'pre') return new HTMLPreElement();
            if (tag === 'code') return new HTMLCodeElement();
            if (tag === 'div') return new HTMLDivElement();
            return new HTMLElement(tag);
        }

        /** @param {string} id */
        getElementById(id) {
            const target = String(id);
            for (const el of collectDescendants(this)) {
                if (el.id === target) return el;
            }
            return null;
        }

        /** @param {string} selector */
        querySelector(selector) {
            const all = this.querySelectorAll(selector);
            return all.length > 0 ? all[0] : null;
        }

        /** @param {string} selector */
        querySelectorAll(selector) {
            return querySelectorAllWithin(this, selector);
        }
    }

    class MutationObserver {
        /** @param {(records: any[]) => void} callback */
        constructor(callback) {
            this._callback = callback;
            createdMutationObservers.push(this);
        }

        /** @type {(records: any[]) => void} */
        _callback;

        observe(target, options) {
            this._target = target;
            this._options = options;
        }

        disconnect() {
            this._target = null;
            this._options = null;
        }

        /** @param {any[]} records */
        _trigger(records) {
            this._callback(records);
        }
    }

    class IntersectionObserver {
        /** @param {(entries: any[]) => void} callback */
        constructor(callback, options) {
            this._callback = callback;
            this._options = options;
            createdIntersectionObservers.push(this);
        }

        /** @type {(entries: any[]) => void} */
        _callback;

        observe() {}
        unobserve() {}
        disconnect() {}

        /** @param {any[]} entries */
        _trigger(entries) {
            this._callback(entries);
        }
    }

    const document = new Document();
    const window = {
        innerWidth,
        innerHeight,
        addEventListener() {},
        removeEventListener() {},
    };

    const localStorageMap = new Map();
    const localStorage = {
        getItem(key) {
            return localStorageMap.has(String(key)) ? localStorageMap.get(String(key)) : null;
        },
        setItem(key, value) {
            localStorageMap.set(String(key), String(value));
        },
        removeItem(key) {
            localStorageMap.delete(String(key));
        },
        clear() {
            localStorageMap.clear();
        },
    };

    patchGlobal('document', document);
    patchGlobal('window', window);
    patchGlobal('Element', FakeElement);
    patchGlobal('HTMLElement', HTMLElement);
    patchGlobal('HTMLDivElement', HTMLDivElement);
    patchGlobal('HTMLPreElement', HTMLPreElement);
    patchGlobal('HTMLTemplateElement', HTMLTemplateElement);
    patchGlobal('HTMLIFrameElement', HTMLIFrameElement);
    patchGlobal('MutationObserver', MutationObserver);
    patchGlobal('IntersectionObserver', IntersectionObserver);
    patchGlobal('innerWidth', innerWidth);
    patchGlobal('innerHeight', innerHeight);
    patchGlobal('performance', { now: () => nowMs });
    patchGlobal('navigator', { userAgent, platform, maxTouchPoints });
    patchGlobal('localStorage', localStorage);

    patchGlobal('queueMicrotask', (fn) => {
        microtasks.push(fn);
    });
    patchGlobal('requestAnimationFrame', (fn) => {
        rafs.push(fn);
        return rafs.length;
    });

    const flushMicrotasks = () => {
        while (microtasks.length) {
            const next = microtasks.shift();
            next?.();
        }
    };

    const flushRaf = () => {
        while (rafs.length) {
            const next = rafs.shift();
            if (typeof next === 'function') {
                next(nowMs);
            }
        }
    };

    return {
        document,
        window,
        createdMutationObservers,
        createdIntersectionObservers,
        flushMicrotasks,
        flushRaf,
        setNowMs: (value) => {
            nowMs = Number(value) || 0;
        },
        cleanup: () => {
            for (const [key, descriptor] of patchedGlobals.entries()) {
                if (descriptor) {
                    Object.defineProperty(globalThis, key, descriptor);
                } else {
                    delete globalThis[key];
                }
            }
        },
    };
}
