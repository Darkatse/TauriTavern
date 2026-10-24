import test from 'node:test';
import assert from 'node:assert/strict';

import { Window } from 'happy-dom';

test('CodeMirror, isolated or in the page, keeps raw input inside, shares edit and replacement history, copies current text, and resets without blurring', async () => {
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
    for (const isolated of [true, false]) {
        const host = document.createElement('div');
        document.body.append(host);
        const copies = [];
        const changes = [];

        const editor = createCodeMirrorView(host, {
            isolated,
            doc: 'first preset',
            readOnly: false,
            ariaLabel: 'Prompt',
            onCopy: value => copies.push(value),
            onChange: () => changes.push(editor.getValue()),
        });
        editor.focus();
        const root = isolated ? host.shadowRoot : host;
        const view = EditorView.findFromDOM(root.querySelector('.cm-editor'));
        let leakedInput = 0;
        const countLeak = () => leakedInput++;
        document.addEventListener('input', countLeak);
        root.querySelector('.cm-content').dispatchEvent(new Event('input', { bubbles: true, composed: true }));
        document.removeEventListener('input', countLeak);
        assert.equal(leakedInput, 0);
        const button = label => root.querySelector(`button[aria-label="${label}"]`);
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
        host.addEventListener('focusout', () => focusOutCount++);
        editor.reset('second preset', true);

        assert.equal(editor.getValue(), 'second preset');
        assert.equal(focusOutCount, 0);
        assert.equal(root.querySelector('.cm-content')?.getAttribute('contenteditable'), 'false');
        assert.equal(button('Undo').disabled, true);
        assert.equal(button('Redo').disabled, true);
        button('Copy all').click();
        assert.deepEqual(copies, ['first preset edited', 'second preset']);
        assert.equal(changes.length, 3);

        editor.reset('Alice meets Alice');
        button('Find and replace').click();
        assert.equal(button('Find and replace').getAttribute('aria-expanded'), 'true');
        assert.equal(host.hasAttribute('data-search-open'), true);
        assert.equal(root.querySelector('.cm-search-replace').hidden, true);
        button('Toggle replace').click();
        assert.equal(root.querySelector('.cm-search-replace').hidden, false);
        const search = root.querySelector('input[name=search]');
        const replace = root.querySelector('input[name=replace]');
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
        assert.equal(root.querySelector('[role=status]').textContent, 'Invalid regular expression');
        button('Close').click();
        assert.equal(root.querySelector('.cm-editor-search'), null);
        assert.equal(button('Find and replace').getAttribute('aria-expanded'), 'false');
        assert.equal(host.hasAttribute('data-search-open'), false);
        assert.equal(view.hasFocus, true);

        editor.reset('Read only', true);
        button('Find and replace').click();
        assert.ok(root.querySelector('input[name=search]'));
        assert.equal(root.querySelector('input[name=replace]'), null);

        if (!isolated) {
            // Every editor mounts the same styles; rewriting the page's style element would restyle the whole page.
            const styles = [...document.head.querySelectorAll('style')].find(style => style.textContent.includes('.cm-editor'));
            const rewrites = new MutationObserver(() => {});
            rewrites.observe(styles, { childList: true, characterData: true, subtree: true });
            const other = document.createElement('div');
            document.body.append(other);
            createCodeMirrorView(other, { isolated, doc: '', readOnly: false, ariaLabel: 'Other', onCopy() {}, onChange() {} }).destroy();
            editor.reset('Read only again', true);
            assert.equal(rewrites.takeRecords().length, 0);
            rewrites.disconnect();
        }

        editor.destroy();
    }
    window.close();
});
