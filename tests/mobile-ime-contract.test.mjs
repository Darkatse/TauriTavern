import assert from 'node:assert/strict';
import test from 'node:test';
import { Window } from 'happy-dom';

let importId = 0;

async function createHarness({ android = true } = {}) {
    const window = new Window({ url: 'https://tauritavern.local/' });
    window.document.body.innerHTML = `
        <div id="sheld"><textarea id="composer"></textarea></div>
        <div id="character_popup">
            <textarea id="editor"></textarea>
            <textarea id="readonly" readonly></textarea>
            <input id="checkbox" type="checkbox">
            <input id="hidden" type="text" hidden>
            <button id="button">Done</button>
        </div>
    `;
    for (const element of window.document.querySelectorAll('textarea, input')) {
        element.getClientRects = () => element.hidden ? [] : [{}];
    }

    const calls = [];
    window.__TAURITAVERN_INSETS__ = {
        setImeTarget: target => calls.push(target),
    };
    Object.assign(globalThis, {
        window,
        document: window.document,
        HTMLElement: window.HTMLElement,
        HTMLInputElement: window.HTMLInputElement,
        HTMLTextAreaElement: window.HTMLTextAreaElement,
        getComputedStyle: window.getComputedStyle.bind(window),
    });
    Object.defineProperty(globalThis, 'navigator', {
        configurable: true,
        value: {
            userAgent: android
                ? 'Mozilla/5.0 (Linux; Android 14) TauriTavern'
                : 'Mozilla/5.0 (X11; Linux x86_64)',
            maxTouchPoints: android ? 5 : 0,
            platform: android ? 'Android' : 'Linux',
        },
    });

    const module = await import(new URL(
        `../src/tauri/main/compat/mobile/mobile-ime-surface-controller.js?test=${++importId}`,
        import.meta.url,
    ).href);
    return { window, calls, install: module.installMobileImeSurfaceController };
}

test('IME focus routing moves between composer and fixed-shell surfaces', async () => {
    const harness = await createHarness();
    const controller = harness.install();
    const { document } = harness.window;
    const composer = document.getElementById('composer');
    const fixedShell = document.getElementById('character_popup');
    const editor = document.getElementById('editor');

    composer.focus();
    assert.equal(document.getElementById('sheld').getAttribute('data-tt-ime-surface'), 'composer');
    assert.deepEqual(harness.calls, []);

    editor.focus();
    assert.equal(fixedShell.getAttribute('data-tt-ime-surface'), 'fixed-shell');
    assert.equal(fixedShell.hasAttribute('data-tt-ime-active'), true);
    assert.equal(harness.calls.at(-1), fixedShell);

    document.getElementById('button').focus();
    assert.equal(fixedShell.hasAttribute('data-tt-ime-active'), false);
    assert.equal(harness.calls.at(-1), null);
    controller.dispose();
    harness.window.close();
});

test('IME routing ignores controls that cannot summon a keyboard', async () => {
    const harness = await createHarness();
    const controller = harness.install();
    const { document } = harness.window;

    for (const id of ['checkbox', 'hidden', 'readonly']) {
        document.getElementById(id).focus();
        assert.equal(document.getElementById('character_popup').hasAttribute('data-tt-ime-active'), false);
    }
    assert.deepEqual(harness.calls, []);
    controller.dispose();
    harness.window.close();
});

test('IME routing does not install outside Android', async () => {
    const harness = await createHarness({ android: false });
    assert.equal(harness.install(), null);
    harness.window.close();
});
