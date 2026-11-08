import { expect, test } from '@rstest/core';
import { EditorView } from '@codemirror/view';

import { createCodeMirrorView } from '../../src/lib-bundle-editor.js';

test('CodeMirror, isolated or in the page, keeps raw input inside, shares edit and replacement history, copies current text, and resets without blurring', () => {
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
        expect(leakedInput).toBe(0);
        const button = label => root.querySelector(`button[aria-label="${label}"]`);
        expect(button('Undo').disabled).toBe(true);
        expect(button('Redo').disabled).toBe(true);

        view.dispatch({ changes: { from: view.state.doc.length, insert: ' edited' } });
        expect(button('Undo').disabled).toBe(false);
        button('Undo').click();
        expect(editor.getValue()).toBe('first preset');
        expect(button('Redo').disabled).toBe(false);
        button('Redo').click();
        expect(editor.getValue()).toBe('first preset edited');
        expect(view.hasFocus).toBe(true);
        button('Copy all').click();
        expect(copies).toEqual(['first preset edited']);
        expect(changes).toEqual(['first preset edited', 'first preset', 'first preset edited']);

        let focusOutCount = 0;
        host.addEventListener('focusout', () => focusOutCount++);
        editor.reset('second preset', true);

        expect(editor.getValue()).toBe('second preset');
        expect(focusOutCount).toBe(0);
        expect(root.querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false');
        expect(button('Undo').disabled).toBe(true);
        expect(button('Redo').disabled).toBe(true);
        button('Copy all').click();
        expect(copies).toEqual(['first preset edited', 'second preset']);
        expect(changes.length).toBe(3);

        editor.reset('Alice meets Alice');
        button('Find and replace').click();
        expect(button('Find and replace').getAttribute('aria-expanded')).toBe('true');
        expect(host.hasAttribute('data-search-open')).toBe(true);
        expect(root.querySelector('.cm-search-replace').hidden).toBe(true);
        button('Toggle replace').click();
        expect(root.querySelector('.cm-search-replace').hidden).toBe(false);
        const search = root.querySelector('input[name=search]');
        const replace = root.querySelector('input[name=replace]');
        search.value = 'Alice';
        search.dispatchEvent(new Event('input', { bubbles: true }));
        replace.value = 'Bob';
        replace.dispatchEvent(new Event('input', { bubbles: true }));
        button('Next').click();
        expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('Alice');
        button('Replace').click();
        expect(editor.getValue()).toBe('Bob meets Alice');
        button('Replace all').click();
        expect(editor.getValue()).toBe('Bob meets Bob');
        button('Undo').click();
        expect(editor.getValue()).toBe('Bob meets Alice');
        button('Redo').click();
        expect(editor.getValue()).toBe('Bob meets Bob');
        expect(changes.at(-1)).toBe('Bob meets Bob');
        search.value = '[';
        search.dispatchEvent(new Event('input', { bubbles: true }));
        button('Regular expression').click();
        expect(search.getAttribute('aria-invalid')).toBe('true');
        expect(button('Replace all').disabled).toBe(true);
        expect(root.querySelector('[role=status]').textContent).toBe('Invalid regular expression');
        button('Close').click();
        expect(root.querySelector('.cm-editor-search')).toBeNull();
        expect(button('Find and replace').getAttribute('aria-expanded')).toBe('false');
        expect(host.hasAttribute('data-search-open')).toBe(false);
        expect(view.hasFocus).toBe(true);

        editor.reset('Read only', true);
        button('Find and replace').click();
        expect(root.querySelector('input[name=search]')).not.toBeNull();
        expect(root.querySelector('input[name=replace]')).toBeNull();

        if (!isolated) {
            // Every editor mounts the same styles; rewriting the page's style element would restyle the whole page.
            const styles = [...document.head.querySelectorAll('style')].find(style => style.textContent.includes('.cm-editor'));
            const rewrites = new MutationObserver(() => {});
            rewrites.observe(styles, { childList: true, characterData: true, subtree: true });
            const other = document.createElement('div');
            document.body.append(other);
            createCodeMirrorView(other, { isolated, doc: '', readOnly: false, ariaLabel: 'Other', onCopy() {}, onChange() {} }).destroy();
            editor.reset('Read only again', true);
            expect(rewrites.takeRecords().length).toBe(0);
            rewrites.disconnect();
        }

        editor.destroy();
    }
});
