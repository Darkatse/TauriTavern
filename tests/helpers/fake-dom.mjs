import { Window } from 'happy-dom';

export function installFakeDom({
    innerWidth = 800,
    innerHeight = 600,
    userAgent = 'node',
    platform = 'node',
    maxTouchPoints = 0,
} = {}) {
    const previousGlobals = new Map();
    const patchGlobal = (name, value) => {
        previousGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
        Object.defineProperty(globalThis, name, {
            configurable: true,
            enumerable: true,
            writable: true,
            value,
        });
    };

    const window = new Window({ url: 'https://tauritavern.local/' });
    Object.defineProperties(window, {
        innerWidth: { configurable: true, value: innerWidth },
        innerHeight: { configurable: true, value: innerHeight },
    });
    Object.defineProperties(window.navigator, {
        userAgent: { configurable: true, value: userAgent },
        platform: { configurable: true, value: platform },
        maxTouchPoints: { configurable: true, value: maxTouchPoints },
    });

    let nowMs = 0;
    const microtasks = [];
    const rafs = [];
    const createdMutationObservers = [];
    const createdIntersectionObservers = [];
    const createdResizeObservers = [];

    class MutationObserver {
        constructor(callback) {
            this._callback = callback;
            createdMutationObservers.push(this);
        }

        observe(target, options) {
            this._target = target;
            this._options = options;
        }

        disconnect() {
            this._target = null;
            this._options = null;
        }

        _trigger(records) {
            this._callback(records);
        }
    }

    class IntersectionObserver {
        constructor(callback, options) {
            this._callback = callback;
            this._options = options;
            createdIntersectionObservers.push(this);
        }

        observe() {}
        unobserve() {}
        disconnect() {}

        _trigger(entries) {
            this._callback(entries);
        }
    }

    class ResizeObserver {
        constructor(callback) {
            this._callback = callback;
            this._targets = new Set();
            createdResizeObservers.push(this);
        }

        observe(target) {
            this._targets.add(target);
        }

        unobserve(target) {
            this._targets.delete(target);
        }

        disconnect() {
            this._targets.clear();
        }

        _trigger(entries) {
            this._callback(entries);
        }
    }

    const rects = new WeakMap();
    const layout = new WeakMap();
    const layoutValue = (element, name) => layout.get(element)?.[name];
    const setLayoutValue = (element, name, value) => {
        const values = layout.get(element) ?? {};
        values[name] = Number(value) || 0;
        layout.set(element, values);
    };
    const elementPrototype = window.HTMLElement.prototype;

    elementPrototype._setRect = function (patch) {
        const rect = {
            top: 0,
            right: 0,
            bottom: 0,
            left: 0,
            width: 0,
            height: 0,
            ...rects.get(this),
            ...patch,
        };
        if (patch.bottom === undefined && (patch.top !== undefined || patch.height !== undefined)) {
            rect.bottom = rect.top + rect.height;
        }
        if (patch.right === undefined && (patch.left !== undefined || patch.width !== undefined)) {
            rect.right = rect.left + rect.width;
        }
        rects.set(this, rect);
        if (patch.width !== undefined) {
            setLayoutValue(this, 'offsetWidth', patch.width);
            setLayoutValue(this, 'clientWidth', patch.width);
        }
        if (patch.height !== undefined) {
            setLayoutValue(this, 'offsetHeight', patch.height);
            setLayoutValue(this, 'clientHeight', patch.height);
        }
    };
    elementPrototype.getBoundingClientRect = function () {
        const rect = rects.get(this) ?? {
            top: 0,
            right: 0,
            bottom: 0,
            left: 0,
            width: 0,
            height: 0,
        };
        return { x: rect.left, y: rect.top, ...rect, toJSON() {} };
    };

    for (const name of ['offsetWidth', 'offsetHeight', 'clientWidth', 'clientHeight']) {
        Object.defineProperty(elementPrototype, name, {
            configurable: true,
            get() {
                return layoutValue(this, name) ?? 0;
            },
            set(value) {
                setLayoutValue(this, name, value);
            },
        });
    }
    Object.defineProperty(elementPrototype, 'scrollHeight', {
        configurable: true,
        get() {
            const explicit = layoutValue(this, 'scrollHeight');
            if (explicit !== undefined) {
                return explicit;
            }
            const gap = Number.parseFloat(this.style.rowGap || '0') || 0;
            return [...this.children]
                .filter(child => !['absolute', 'fixed'].includes(child.style.position) && child.style.display !== 'none')
                .reduce((height, child, index) => (
                    height + child.getBoundingClientRect().height + (index > 0 ? gap : 0)
                ), 0);
        },
        set(value) {
            setLayoutValue(this, 'scrollHeight', value);
        },
    });

    const queueMicrotask = callback => microtasks.push(callback);
    const requestAnimationFrame = callback => {
        rafs.push(callback);
        return rafs.length;
    };
    const cancelAnimationFrame = id => {
        if (id > 0 && id <= rafs.length) {
            rafs[id - 1] = null;
        }
    };
    Object.assign(window, {
        MutationObserver,
        IntersectionObserver,
        ResizeObserver,
        queueMicrotask,
        requestAnimationFrame,
        cancelAnimationFrame,
    });

    const zeroPixelProperties = new Set([
        'paddingBlockStart',
        'paddingBlockEnd',
        'paddingTop',
        'paddingBottom',
        'marginBlockStart',
        'marginBlockEnd',
        'marginTop',
        'marginBottom',
        'rowGap',
    ]);
    const getComputedStyle = element => new Proxy(window.getComputedStyle(element), {
        get(target, property) {
            const value = target[property];
            if (zeroPixelProperties.has(property) && !value) {
                return '0px';
            }
            return typeof value === 'function' ? value.bind(target) : value;
        },
    });

    const globals = {
        window,
        document: window.document,
        navigator: window.navigator,
        localStorage: window.localStorage,
        Node: window.Node,
        Element: window.Element,
        HTMLElement: window.HTMLElement,
        HTMLDivElement: window.HTMLDivElement,
        HTMLPreElement: window.HTMLPreElement,
        HTMLTemplateElement: window.HTMLTemplateElement,
        HTMLIFrameElement: window.HTMLIFrameElement,
        DocumentFragment: window.DocumentFragment,
        MutationObserver,
        IntersectionObserver,
        ResizeObserver,
        innerWidth,
        innerHeight,
        performance: { now: () => nowMs },
        getComputedStyle,
        queueMicrotask,
        requestAnimationFrame,
        cancelAnimationFrame,
    };
    for (const [name, value] of Object.entries(globals)) {
        patchGlobal(name, value);
    }

    return {
        document: window.document,
        window,
        createdMutationObservers,
        createdIntersectionObservers,
        createdResizeObservers,
        flushMicrotasks() {
            while (microtasks.length) {
                microtasks.shift()?.();
            }
        },
        flushRaf() {
            while (rafs.length) {
                rafs.shift()?.(nowMs);
            }
        },
        setNowMs(value) {
            nowMs = Number(value) || 0;
        },
        cleanup() {
            void window.happyDOM.abort();
            for (const [name, descriptor] of previousGlobals) {
                if (descriptor) {
                    Object.defineProperty(globalThis, name, descriptor);
                } else {
                    delete globalThis[name];
                }
            }
        },
    };
}
