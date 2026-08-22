import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Window } from 'happy-dom';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');


test('keyboard observer limits subtree work to structural focusability changes', async () => {
    const window = new Window();
    Object.assign(globalThis, {
        document: window.document,
        CSS: window.CSS,
        Element: window.Element,
        HTMLElement: window.HTMLElement,
        MutationObserver: window.MutationObserver,
    });

    try {
        const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/scripts/keyboard.js'));
        const { initKeyboard, registerInteractableType } = await import(`${moduleUrl.href}?test`);
        initKeyboard();

        const wrapper = document.createElement('div');
        const button = document.createElement('div');
        button.className = 'menu_button';
        wrapper.append(button);
        document.body.append(wrapper);
        await window.happyDOM.waitUntilComplete();

        assert.equal(button.getAttribute('tabindex'), '0');

        const extensionControl = document.createElement('div');
        extensionControl.className = 'extension-control';
        document.body.append(extensionControl);
        await window.happyDOM.waitUntilComplete();
        registerInteractableType('.extension-control');
        assert.equal(extensionControl.getAttribute('tabindex'), '0');
        assert.equal(extensionControl.classList.contains('custom_interactable'), false);

        const querySelectorAll = wrapper.querySelectorAll.bind(wrapper);
        let subtreeQueries = 0;
        wrapper.querySelectorAll = (...args) => {
            subtreeQueries += 1;
            return querySelectorAll(...args);
        };

        wrapper.classList.add('visual-state');
        await window.happyDOM.waitUntilComplete();
        assert.equal(subtreeQueries, 0);

        wrapper.classList.add('disabled');
        await window.happyDOM.waitUntilComplete();
        assert.equal(subtreeQueries, 1);
        assert.equal(button.hasAttribute('tabindex'), false);
        assert.equal(button.getAttribute('data-original-tabindex'), '0');

        wrapper.classList.remove('disabled');
        await window.happyDOM.waitUntilComplete();
        assert.equal(subtreeQueries, 2);
        assert.equal(button.getAttribute('tabindex'), '0');

        const scrollContainer = document.createElement('div');
        scrollContainer.className = 'scroll-reset-container';
        const addEventListener = scrollContainer.addEventListener.bind(scrollContainer);
        let focusoutListeners = 0;
        scrollContainer.addEventListener = (type, listener, options) => {
            if (type === 'focusout') {
                focusoutListeners += 1;
            }
            return addEventListener(type, listener, options);
        };

        document.body.append(scrollContainer);
        await window.happyDOM.waitUntilComplete();
        scrollContainer.classList.add('expanded');
        await window.happyDOM.waitUntilComplete();
        scrollContainer.remove();
        document.body.append(scrollContainer);
        await window.happyDOM.waitUntilComplete();

        assert.equal(focusoutListeners, 1);
    } finally {
        window.close();
        for (const name of ['document', 'CSS', 'Element', 'HTMLElement', 'MutationObserver']) {
            delete globalThis[name];
        }
    }
});
