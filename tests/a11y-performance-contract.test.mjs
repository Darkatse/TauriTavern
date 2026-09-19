import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Window } from 'happy-dom';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('a11y observer scans each added subtree once', async () => {
    const window = new Window();
    Object.assign(globalThis, {
        document: window.document,
        Element: window.Element,
        MutationObserver: window.MutationObserver,
    });

    try {
        const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/scripts/a11y.js'));
        const { initAccessibility } = await import(`${moduleUrl.href}?test`);
        initAccessibility();

        const list = document.createElement('div');
        list.className = 'list-group';
        const button = document.createElement('button');
        button.className = 'menu_button';
        list.append(button);

        const querySelectorAll = list.querySelectorAll.bind(list);
        let subtreeQueries = 0;
        list.querySelectorAll = (...args) => {
            subtreeQueries += 1;
            return querySelectorAll(...args);
        };

        document.body.append(list);
        await window.happyDOM.waitUntilComplete();

        assert.equal(subtreeQueries, 1);
        assert.equal(list.getAttribute('role'), 'list');
        assert.equal(button.getAttribute('role'), 'button');
    } finally {
        window.close();
        for (const name of ['document', 'Element', 'MutationObserver']) {
            delete globalThis[name];
        }
    }
});
