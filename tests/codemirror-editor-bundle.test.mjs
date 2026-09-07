import test from 'node:test';
import assert from 'node:assert/strict';

import { Window } from 'happy-dom';

test('CodeMirror toolbar shares edit and replacement history, copies current text, and resets without blurring', async () => {
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
    const { EditorView } = await import('@codemirror/view');
    const parent = document.createElement('div');
    document.body.append(parent);
    const copies = [];
    const changes = [];

    const editor = createCodeMirrorView(parent, {
        doc: 'first preset',
        readOnly: false,
        ariaLabel: 'Prompt',
        onCopy: value => copies.push(value),
        onChange: () => changes.push(editor.getValue()),
    });
    editor.focus();
    const view = EditorView.findFromDOM(parent.querySelector('.cm-editor'));
    const button = label => parent.querySelector(`button[aria-label="${label}"]`);
    assert.equal(button('Undo').disabled, true);
    assert.equal(button('Redo').disabled, true);

    view.dispatch({ changes: { from: view.state.doc.length, insert: ' edited' } });
    assert.equal(button('Undo').disabled, false);
    button('Undo').click();
    assert.equal(editor.getValue(), 'first preset');
    assert.equal(button('Redo').disabled, false);
    button('Redo').click();
    assert.equal(editor.getValue(), 'first preset edited');
    assert.equal(view.hasFocus, true);
    button('Copy all').click();
    assert.deepEqual(copies, ['first preset edited']);
    assert.deepEqual(changes, ['first preset edited', 'first preset', 'first preset edited']);

    let focusOutCount = 0;
    parent.addEventListener('focusout', () => focusOutCount++);
    editor.reset('second preset', true);

    assert.equal(editor.getValue(), 'second preset');
    assert.equal(focusOutCount, 0);
    assert.equal(parent.querySelector('.cm-content')?.getAttribute('contenteditable'), 'false');
    assert.equal(button('Undo').disabled, true);
    assert.equal(button('Redo').disabled, true);
    button('Copy all').click();
    assert.deepEqual(copies, ['first preset edited', 'second preset']);
    assert.equal(changes.length, 3);

    editor.reset('Alice meets Alice');
    button('Find and replace').click();
    assert.equal(button('Find and replace').getAttribute('aria-expanded'), 'true');
    assert.equal(parent.querySelector('.cm-search-replace').hidden, true);
    button('Toggle replace').click();
    assert.equal(parent.querySelector('.cm-search-replace').hidden, false);
    const search = parent.querySelector('input[name=search]');
    const replace = parent.querySelector('input[name=replace]');
    search.value = 'Alice';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    replace.value = 'Bob';
    replace.dispatchEvent(new Event('input', { bubbles: true }));
    button('Next').click();
    assert.equal(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to), 'Alice');
    button('Replace').click();
    assert.equal(editor.getValue(), 'Bob meets Alice');
    button('Replace all').click();
    assert.equal(editor.getValue(), 'Bob meets Bob');
    button('Undo').click();
    assert.equal(editor.getValue(), 'Bob meets Alice');
    button('Redo').click();
    assert.equal(editor.getValue(), 'Bob meets Bob');
    assert.equal(changes.at(-1), 'Bob meets Bob');
    search.value = '[';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    button('Regular expression').click();
    assert.equal(search.getAttribute('aria-invalid'), 'true');
    assert.equal(button('Replace all').disabled, true);
    assert.equal(parent.querySelector('[role=status]').textContent, 'Invalid regular expression');
    button('Close').click();
    assert.equal(parent.querySelector('.cm-editor-search'), null);
    assert.equal(button('Find and replace').getAttribute('aria-expanded'), 'false');
    assert.equal(view.hasFocus, true);

    editor.reset('Read only', true);
    button('Find and replace').click();
    assert.ok(parent.querySelector('input[name=search]'));
    assert.equal(parent.querySelector('input[name=replace]'), null);

    editor.destroy();
    window.close();
});
