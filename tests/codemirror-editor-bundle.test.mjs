import test from 'node:test';
import assert from 'node:assert/strict';

import { Window } from 'happy-dom';

test('CodeMirror reset replaces the document without blurring the editor', async () => {
    const window = new Window();
    for (const key of ['window', 'document', 'navigator', 'HTMLElement', 'MutationObserver', 'ResizeObserver', 'DOMRect', 'Node', 'Event']) {
        Object.defineProperty(globalThis, key, {
            value: window[key] ?? window.document.defaultView[key],
            configurable: true,
        });
    }
    globalThis.requestAnimationFrame = callback => setTimeout(() => callback(Date.now()), 0);
    globalThis.cancelAnimationFrame = clearTimeout;
    globalThis.getComputedStyle = window.getComputedStyle.bind(window);

    const { createCodeMirrorView } = await import('../src/lib-bundle-editor.js');
    const parent = document.createElement('div');
    document.body.append(parent);
    let focusOutCount = 0;
    parent.addEventListener('focusout', () => focusOutCount++);

    const editor = createCodeMirrorView(parent, {
        doc: 'first preset',
        readOnly: false,
        ariaLabel: 'Prompt',
    });
    editor.focus();
    editor.reset('second preset', true);

    assert.equal(editor.getValue(), 'second preset');
    assert.equal(focusOutCount, 0);
    assert.equal(parent.querySelector('.cm-content')?.getAttribute('contenteditable'), 'false');

    editor.destroy();
    window.close();
});
