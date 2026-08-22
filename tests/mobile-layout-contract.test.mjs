import assert from 'node:assert/strict';
import test from 'node:test';
import { Window } from 'happy-dom';

import {
    applySurfaceContract,
    HOST_ADMITTED_ATTR,
    ORIGINAL_TOP_VAR,
    SURFACE_ATTR,
} from '../src/tauri/main/compat/mobile/mobile-overlay-surface-admission.js';
import { installMobileIframeViewportContractBridge } from '../src/tauri/main/compat/mobile/mobile-iframe-viewport-contract-bridge.js';

function installDom() {
    const window = new Window({ url: 'https://tauritavern.local/' });
    Object.defineProperties(window, {
        innerWidth: { configurable: true, value: 390 },
        innerHeight: { configurable: true, value: 844 },
        visualViewport: {
            configurable: true,
            value: {
                width: 390,
                height: 844,
                addEventListener() {},
                removeEventListener() {},
            },
        },
    });
    Object.assign(globalThis, {
        window,
        document: window.document,
        HTMLElement: window.HTMLElement,
        HTMLIFrameElement: window.HTMLIFrameElement,
        MutationObserver: window.MutationObserver,
        getComputedStyle: window.getComputedStyle.bind(window),
        requestAnimationFrame: callback => {
            callback(0);
            return 1;
        },
    });
    for (const [name, value] of [
        ['--tt-inset-top', '24px'],
        ['--tt-inset-left', '0px'],
        ['--tt-inset-right', '0px'],
        ['--tt-inset-bottom', '20px'],
        ['--tt-viewport-bottom-inset', '20px'],
        ['--tt-base-viewport-height', '844px'],
    ]) {
        window.document.documentElement.style.setProperty(name, value);
    }
    return window;
}

function fixedElement(window, {
    tagName = 'div',
    id = '',
    className = '',
    rect,
    style = {},
    scriptId = false,
}) {
    const element = window.document.createElement(tagName);
    element.id = id;
    element.className = className;
    Object.assign(element.style, {
        position: 'fixed',
        display: 'block',
        visibility: 'visible',
        pointerEvents: 'auto',
        top: '0px',
        cursor: 'default',
        touchAction: 'auto',
        ...style,
    });
    if (scriptId) {
        element.setAttribute('script_id', 'extension/example');
    }
    element.getBoundingClientRect = () => ({
        x: rect.left,
        y: rect.top,
        ...rect,
        toJSON() {},
    });
    window.document.body.append(element);
    return element;
}

test('mobile overlays are classified from observable geometry and affordances', () => {
    const window = installDom();
    const cases = [
        {
            expected: 'fullscreen-window',
            element: fixedElement(window, {
                rect: { top: 0, left: 0, right: 390, bottom: 844, width: 390, height: 844 },
            }),
        },
        {
            expected: 'backdrop',
            element: fixedElement(window, {
                id: 'dialog-overlay',
                rect: { top: 0, left: 0, right: 390, bottom: 844, width: 390, height: 844 },
            }),
        },
        {
            expected: 'viewport-host',
            element: fixedElement(window, {
                tagName: 'iframe',
                scriptId: true,
                rect: { top: 0, left: 0, right: 390, bottom: 844, width: 390, height: 844 },
            }),
        },
        {
            expected: 'free-window',
            element: fixedElement(window, {
                rect: { top: 10, left: 20, right: 220, bottom: 210, width: 200, height: 200 },
                style: { cursor: 'move', top: '10px' },
            }),
        },
        {
            expected: 'edge-window',
            originalTop: '40px',
            element: fixedElement(window, {
                rect: { top: 40, left: 20, right: 220, bottom: 240, width: 200, height: 200 },
                style: { top: '40px' },
            }),
        },
    ];

    for (const { element, expected, originalTop = '' } of cases) {
        applySurfaceContract(element);
        assert.equal(element.getAttribute(SURFACE_ATTR), expected);
        assert.equal(element.getAttribute(HOST_ADMITTED_ATTR), '1');
        assert.equal(element.style.getPropertyValue(ORIGINAL_TOP_VAR), originalTop);
    }
    window.close();
});

test('mobile overlay admission is revoked when a surface becomes hidden', () => {
    const window = installDom();
    const element = fixedElement(window, {
        rect: { top: 10, left: 20, right: 220, bottom: 210, width: 200, height: 200 },
        style: { cursor: 'move', top: '10px' },
    });

    applySurfaceContract(element);
    element.style.display = 'none';
    applySurfaceContract(element);

    assert.equal(element.hasAttribute(SURFACE_ATTR), false);
    assert.equal(element.hasAttribute(HOST_ADMITTED_ATTR), false);
    window.close();
});

test('iframe viewport bridge copies the current safe-area contract', () => {
    const window = installDom();
    const iframe = window.document.createElement('iframe');
    window.document.body.append(iframe);
    const controller = installMobileIframeViewportContractBridge();

    controller.watchIframe(iframe);
    controller.reapply();
    assert.equal(
        iframe.contentDocument.documentElement.style.getPropertyValue('--tt-inset-top'),
        '24px',
    );

    window.document.documentElement.style.setProperty('--tt-inset-top', '32px');
    controller.reapply();
    assert.equal(
        iframe.contentDocument.documentElement.style.getPropertyValue('--tt-inset-top'),
        '32px',
    );
    controller.dispose();
    window.close();
});
