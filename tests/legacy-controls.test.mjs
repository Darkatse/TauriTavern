import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { getByRole, queryByRole } from '@testing-library/dom';
import { Window } from 'happy-dom';

async function withControls(html, run) {
    const window = new Window();
    for (const name of ['document', 'Element', 'HTMLElement', 'MutationObserver']) globalThis[name] = window[name];
    document.body.innerHTML = html;
    try {
        const accessibility = await import('../src/scripts/a11y.js');
        const keyboard = await import('../src/scripts/keyboard.js');
        accessibility.initAccessibility();
        keyboard.initKeyboard();
        accessibility.initAccessibility();
        await window.happyDOM.waitUntilComplete();
        const key = (target, type, value, options = {}) => {
            const event = new window.KeyboardEvent(type, { key: value, bubbles: true, cancelable: true, ...options });
            target.dispatchEvent(event);
            return event;
        };
        await run({ window, key, ...keyboard });
    } finally {
        window.close();
        for (const name of ['document', 'Element', 'HTMLElement', 'MutationObserver']) delete globalThis[name];
    }
}

test('legacy roles preserve native, explicit and collection semantics while keeping click-only buttons operable', async () => {
    await withControls(`
        <div class="list-group"><div id="row" class="interactable tt-control-shell">
            <button class="sr-only">Character</button><div id="action" class="menu_button list-group-item" role="button">Action</div>
        </div></div>
        <a id="link" class="menu_button" href="#destination">Link</a>
        <button id="native" class="menu_button">Native</button>
        <h2 id="heading" class="interactable">Heading</h2>
        <div id="switch" class="menu_button" role="switch" aria-checked="false">Switch</div>
        <div class="tags"><span id="passiveTag" class="tag">Passive</span><span id="activeTag" class="tag interactable">Filter</span></div>
        <div id="bg_tabs"><ul class="bg_tabs_list" role="tablist"><li id="tab" class="bg_tab_button" role="tab" tabindex="-1">Tab</li></ul></div>
    `, ({ key }) => {
        assert.equal(document.querySelector('.list-group').hasAttribute('role'), false);
        assert.equal(document.getElementById('action').getAttribute('role'), 'button');
        assert.equal(document.getElementById('switch').getAttribute('role'), 'switch');
        for (const id of ['link', 'native', 'heading']) assert.equal(document.getElementById(id).hasAttribute('role'), false);
        assert.equal(document.getElementById('row').hasAttribute('role'), false);
        assert.equal(document.getElementById('row').hasAttribute('tabindex'), false);
        assert.equal(document.getElementById('passiveTag').hasAttribute('role'), false);
        assert.equal(document.getElementById('passiveTag').hasAttribute('tabindex'), false);
        assert.equal(document.getElementById('activeTag').getAttribute('role'), 'button');
        assert.equal(document.getElementById('tab').getAttribute('tabindex'), '-1');
        assert.equal(document.getElementById('tab').classList.contains('interactable'), false);
        let clicks = 0;
        const action = document.getElementById('action');
        action.addEventListener('click', () => clicks++);
        action.focus();
        key(action, 'keydown', 'Enter');
        assert.equal(clicks, 1);
        key(action, 'keydown', ' ');
        key(action, 'keyup', ' ');
        assert.equal(clicks, 2);
    });
});

test('extension menu entries retain their list owner and do not turn their groups into buttons', async () => {
    const menuTemplate = readFileSync(new URL('../src/scripts/templates/wandMenu.html', import.meta.url), 'utf8');
    const entryTemplate = readFileSync(new URL('../src/scripts/extensions/attachments/manage-button.html', import.meta.url), 'utf8');
    await withControls(menuTemplate, async ({ window, key }) => {
        const menu = document.getElementById('extensionsMenu');
        menu.style.display = '';
        assert.equal(getByRole(document.body, 'list'), menu);
        const group = document.getElementById('data_bank_wand_container');
        group.innerHTML = entryTemplate;
        await window.happyDOM.waitUntilComplete();
        assert.equal(group.hasAttribute('role'), false);
        assert.equal(group.hasAttribute('tabindex'), false);
        const action = getByRole(menu, 'button', { name: 'Open Data Bank' });
        let clicks = 0;
        action.addEventListener('click', () => clicks++);
        action.focus();
        key(action, 'keydown', 'Enter');
        assert.equal(clicks, 1);

        // JS-Slash-Runner authors listitems; MemoryBooks also opts its wrapper into focus.
        for (const attributes of ['', 'class="interactable" tabindex="0"']) {
            const extension = document.createElement('div');
            extension.innerHTML = `<div ${attributes}><div class="list-group-item interactable" role="listitem" tabindex="0">
                <div class="extensionsMenuExtensionButton"></div><span>Extension action</span>
            </div></div>`;
            const container = extension.firstElementChild;
            container.classList.add('extension_container');
            menu.append(container);
            await window.happyDOM.waitUntilComplete();
            assert.equal(container.hasAttribute('tabindex'), Boolean(attributes));
            assert.ok(!queryByRole(menu, 'button', { name: 'Extension action' }), 'The group must not appear as an action');
            const item = getByRole(container, 'listitem');
            assert.equal(item.parentElement.closest('[role="list"]'), menu);
            let opened = 0;
            item.addEventListener('click', () => opened++);
            item.focus();
            key(item, 'keydown', 'Enter');
            assert.equal(opened, 1);
        }
    });
});

test('native and independent editable children receive no synthetic ancestor activation', async () => {
    await withControls(`
        <div id="parent" class="menu_button">
            <input id="input"><textarea id="textarea"></textarea>
            <div id="editable" class="interactable" contenteditable="true"><span id="editableChild">Text</span></div>
            <button id="native" class="menu_button"><span id="buttonIcon" class="inline-drawer-icon">Native</span></button>
            <a id="link" class="menu_button" href="#destination"><i id="linkIcon" class="menu_button">Link</i></a>
            <details><summary id="summary"><span id="summaryIcon" class="drawer-icon">Disclosure</span>
                <input id="summaryInput" class="interactable" tabindex="0">
                <label id="summaryLabel" class="menu_button" for="summaryCheckbox">Toggle</label><input id="summaryCheckbox" type="checkbox" hidden>
            </summary></details>
            <span id="independent" tabindex="0">Independent</span>
        </div>
    `, async ({ key, window }) => {
        let clicks = 0;
        document.getElementById('parent').addEventListener('click', () => clicks++);
        for (const id of ['buttonIcon', 'linkIcon', 'summaryIcon']) {
            const icon = document.getElementById(id);
            assert.equal(icon.hasAttribute('role'), false);
            assert.equal(icon.hasAttribute('tabindex'), false);
        }
        for (const id of ['input', 'textarea', 'editable', 'editableChild', 'native', 'link', 'independent', 'buttonIcon', 'linkIcon', 'summary', 'summaryIcon', 'summaryInput']) {
            const control = document.getElementById(id);
            key(control, 'keydown', 'Enter');
            key(control, 'keydown', ' ');
            key(control, 'keyup', ' ');
        }
        // Synthetic events in happy-dom verify only our handler, not browser defaults.
        assert.equal(clicks, 0);
        assert.equal(document.getElementById('summaryInput').getAttribute('tabindex'), '0');
        const label = document.getElementById('summaryLabel');
        label.focus();
        key(label, 'keydown', ' ');
        key(label, 'keyup', ' ');
        assert.equal(document.getElementById('summaryCheckbox').checked, true);

        const movedIcon = document.createElement('div');
        movedIcon.className = 'inline-drawer-icon';
        document.body.append(movedIcon);
        await window.happyDOM.waitUntilComplete();
        assert.equal(movedIcon.getAttribute('tabindex'), '0');
        document.getElementById('native').append(movedIcon);
        await window.happyDOM.waitUntilComplete();
        assert.equal(movedIcon.hasAttribute('role'), false);
        assert.equal(movedIcon.hasAttribute('tabindex'), false);
    });
});

test('Space commits once on release and cancellation, composition, modifiers or lost focus do not activate', async () => {
    await withControls('<div id="action" class="menu_button">Action</div><input id="other">', ({ key }) => {
        const action = document.getElementById('action');
        const other = document.getElementById('other');
        let clicks = 0;
        action.addEventListener('click', () => clicks++);
        action.focus();
        assert.equal(key(action, 'keydown', ' ').defaultPrevented, true);
        key(action, 'keydown', ' ', { repeat: true });
        assert.equal(clicks, 0);
        key(action, 'keyup', ' ');
        key(action, 'keyup', ' ');
        assert.equal(clicks, 1);
        key(action, 'keydown', ' ');
        other.focus();
        action.focus();
        key(action, 'keyup', ' ');
        assert.equal(clicks, 1);
        for (const options of [{ isComposing: true }, { altKey: true }, { ctrlKey: true }, { shiftKey: true }, { metaKey: true }]) {
            key(action, 'keydown', 'Enter', options);
            key(action, 'keydown', ' ', options);
            key(action, 'keyup', ' ', options);
        }
        action.addEventListener('keydown', event => event.preventDefault());
        key(action, 'keydown', 'Enter');
        key(action, 'keydown', ' ');
        key(action, 'keyup', ' ');
        assert.equal(clicks, 1);
    });
});

test('business ancestor disabled state leaves child actions reachable while control state blocks activation', async () => {
    await withControls(`
        <div id="group" class="group_member disabled"><div id="enable" class="right_menu_button">Enable</div></div>
        <div id="disabled" class="menu_button disabled">Disabled</div>
        <div id="aria" class="menu_button" aria-disabled="true">Unavailable</div>
        <button id="native" disabled>Unavailable</button>
    `, async ({ key, window }) => {
        const enable = document.getElementById('enable');
        let clicks = 0;
        for (const id of ['enable', 'disabled', 'aria', 'native']) {
            const control = document.getElementById(id);
            control.addEventListener('click', () => clicks++);
            key(control, 'keydown', 'Enter');
        }
        assert.equal(enable.getAttribute('tabindex'), '0');
        assert.equal(clicks, 1);
        enable.focus();
        key(enable, 'keydown', ' ');
        enable.setAttribute('aria-disabled', 'true');
        key(enable, 'keyup', ' ');
        await window.happyDOM.waitUntilComplete();
        assert.equal(clicks, 1);
        assert.equal(enable.hasAttribute('tabindex'), false);
        enable.removeAttribute('aria-disabled');
        await window.happyDOM.waitUntilComplete();
        assert.equal(enable.getAttribute('tabindex'), '0');

        const group = document.getElementById('group');
        group.classList.add('not_focusable');
        await window.happyDOM.waitUntilComplete();
        assert.equal(enable.hasAttribute('tabindex'), false);
        group.classList.remove('not_focusable');
        await window.happyDOM.waitUntilComplete();
        assert.equal(enable.getAttribute('tabindex'), '0');
    });
});

test('label proxies retain checkbox, file and submit activation without taking over label semantics', async () => {
    await withControls(`
        <label id="checkboxLabel" class="menu_button" for="checkbox">Toggle</label><input id="checkbox" type="checkbox" hidden>
        <label id="fileLabel" class="menu_button" for="file">Upload</label><input id="file" type="file" hidden>
        <form><label id="submitLabel" class="menu_button" for="submit">Submit</label><input id="submit" type="submit" hidden></form>
    `, async ({ key, window }) => {
        document.querySelector('form').addEventListener('submit', event => event.preventDefault());
        for (const kind of ['checkbox', 'file', 'submit']) {
            const label = document.getElementById(`${kind}Label`);
            const control = document.getElementById(kind);
            let clicks = 0;
            control.addEventListener('click', () => clicks++);
            assert.equal(label.hasAttribute('role'), false);
            assert.equal(label.getAttribute('tabindex'), '0');
            label.focus();
            key(label, 'keydown', 'Enter');
            key(label, 'keydown', ' ');
            key(label, 'keyup', ' ');
            assert.equal(clicks, 2);
            control.disabled = true;
            key(label, 'keydown', 'Enter');
            assert.equal(clicks, 2);
            await window.happyDOM.waitUntilComplete();
            assert.equal(label.hasAttribute('tabindex'), false);
        }
    });
});

test('registered extension actions are discovered dynamically and native transitions stop synthetic activation', async () => {
    await withControls('<div id="extension" class="extension-action">Extension</div><a id="changing" class="menu_button"><i id="changingIcon" class="menu_button">Action</i></a>', async ({ key, window, registerInteractableType }) => {
        registerInteractableType('.extension-action');
        const extension = document.getElementById('extension');
        const later = document.createElement('div');
        later.className = 'extension-action';
        document.body.append(later);
        await window.happyDOM.waitUntilComplete();
        for (const action of [extension, later]) {
            let clicks = 0;
            action.addEventListener('click', () => clicks++);
            action.focus();
            assert.equal(document.activeElement, action);
            key(action, 'keydown', 'Enter');
            assert.equal(clicks, 1);
        }
        const changing = document.getElementById('changing');
        changing.setAttribute('href', '#destination');
        await window.happyDOM.waitUntilComplete();
        assert.equal(changing.hasAttribute('role'), false);
        assert.equal(changing.hasAttribute('tabindex'), false);
        assert.equal(document.getElementById('changingIcon').hasAttribute('role'), false);
        assert.equal(document.getElementById('changingIcon').hasAttribute('tabindex'), false);
        let clicks = 0;
        changing.addEventListener('click', () => clicks++);
        key(changing, 'keydown', 'Enter');
        assert.equal(clicks, 0);
    });
});
