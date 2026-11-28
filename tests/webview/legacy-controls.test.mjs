import { beforeAll, expect, test } from '@rstest/core';
import { page } from '@rstest/browser';
import { getByRole, queryByRole } from '@testing-library/dom';
import { frontendText, importFrontend } from '../helpers/frontend.mjs';

const menuTemplate = await frontendText('scripts/templates/wandMenu.html');
const entryTemplate = await frontendText('scripts/extensions/attachments/manage-button.html');
const accessibility = await importFrontend('scripts/a11y.js');
const { initKeyboard, registerInteractableType } = await importFrontend('scripts/keyboard.js');

beforeAll(() => {
    accessibility.initAccessibility();
    initKeyboard();
    accessibility.initAccessibility();
});

const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));

async function render(html) {
    document.body.innerHTML = html;
    await nextTask();
}

function key(target, type, value, options = {}) {
    const event = new KeyboardEvent(type, { key: value, bubbles: true, cancelable: true, ...options });
    target.dispatchEvent(event);
    return event;
}

test('legacy roles preserve native, explicit and collection semantics while keeping click-only buttons operable', async () => {
    await render(`
        <div class="list-group"><div id="row" class="interactable tt-control-shell">
            <button class="sr-only">Character</button><div id="action" class="menu_button list-group-item" role="button">Action</div>
        </div></div>
        <a id="link" class="menu_button" href="#destination">Link</a>
        <button id="native" class="menu_button">Native</button>
        <h2 id="heading" class="interactable">Heading</h2>
        <div id="switch" class="menu_button" role="switch" aria-checked="false">Switch</div>
        <div class="tags"><span id="passiveTag" class="tag">Passive</span><span id="activeTag" class="tag interactable">Filter</span></div>
        <div id="bg_tabs"><ul class="bg_tabs_list" role="tablist"><li id="tab" class="bg_tab_button" role="tab" tabindex="-1">Tab</li></ul></div>
    `);
    expect(document.querySelector('.list-group').hasAttribute('role')).toBe(false);
    expect(document.getElementById('action').getAttribute('role')).toBe('button');
    expect(document.getElementById('switch').getAttribute('role')).toBe('switch');
    for (const id of ['link', 'native', 'heading']) expect(document.getElementById(id).hasAttribute('role')).toBe(false);
    expect(document.getElementById('row').hasAttribute('role')).toBe(false);
    expect(document.getElementById('row').hasAttribute('tabindex')).toBe(false);
    expect(document.getElementById('passiveTag').hasAttribute('role')).toBe(false);
    expect(document.getElementById('passiveTag').hasAttribute('tabindex')).toBe(false);
    expect(document.getElementById('activeTag').getAttribute('role')).toBe('button');
    expect(document.getElementById('tab').getAttribute('tabindex')).toBe('-1');
    expect(document.getElementById('tab').classList.contains('interactable')).toBe(false);
    let clicks = 0;
    const action = document.getElementById('action');
    action.addEventListener('click', () => clicks++);
    action.focus();
    key(action, 'keydown', 'Enter');
    expect(clicks).toBe(1);
    key(action, 'keydown', ' ');
    key(action, 'keyup', ' ');
    expect(clicks).toBe(2);
});

test('extension menu entries retain their list owner and do not turn their groups into buttons', async () => {
    await render(menuTemplate);
    const menu = document.getElementById('extensionsMenu');
    menu.style.display = '';
    expect(getByRole(document.body, 'list')).toBe(menu);
    const group = document.getElementById('data_bank_wand_container');
    group.innerHTML = entryTemplate;
    await nextTask();
    expect(group.hasAttribute('role')).toBe(false);
    expect(group.hasAttribute('tabindex')).toBe(false);
    const action = getByRole(menu, 'button', { name: 'Open Data Bank' });
    let clicks = 0;
    action.addEventListener('click', () => clicks++);
    action.focus();
    key(action, 'keydown', 'Enter');
    expect(clicks).toBe(1);

    // JS-Slash-Runner authors listitems; MemoryBooks also opts its wrapper into focus.
    for (const attributes of ['', 'class="interactable" tabindex="0"']) {
        const extension = document.createElement('div');
        extension.innerHTML = `<div ${attributes}><div class="list-group-item interactable" role="listitem" tabindex="0">
            <div class="extensionsMenuExtensionButton"></div><span>Extension action</span>
        </div></div>`;
        const container = extension.firstElementChild;
        container.classList.add('extension_container');
        menu.append(container);
        await nextTask();
        expect(container.hasAttribute('tabindex')).toBe(Boolean(attributes));
        expect(queryByRole(menu, 'button', { name: 'Extension action' }), 'The group must not appear as an action').toBeNull();
        const item = getByRole(container, 'listitem');
        expect(item.parentElement.closest('[role="list"]')).toBe(menu);
        let opened = 0;
        item.addEventListener('click', () => opened++);
        item.focus();
        key(item, 'keydown', 'Enter');
        expect(opened).toBe(1);
    }
});

test('native and independent editable children receive no synthetic ancestor activation', async () => {
    await render(`
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
    `);
    const clicks = [];
    document.getElementById('parent').addEventListener('click', event => clicks.push(event.isTrusted));
    for (const id of ['buttonIcon', 'linkIcon', 'summaryIcon']) {
        const icon = document.getElementById(id);
        expect(icon.hasAttribute('role')).toBe(false);
        expect(icon.hasAttribute('tabindex')).toBe(false);
    }
    for (const id of ['input', 'textarea', 'editable', 'editableChild', 'native', 'link', 'independent', 'buttonIcon', 'linkIcon', 'summary', 'summaryIcon', 'summaryInput']) {
        const control = document.getElementById(id);
        key(control, 'keydown', 'Enter');
        key(control, 'keydown', ' ');
        key(control, 'keyup', ' ');
    }
    // Synthetic events, as extensions dispatch them, reach only our handler.
    expect(clicks).toEqual([]);
    // Trusted keys run the browser's own activation, which our handler must not repeat.
    for (const id of ['native', 'link', 'summary']) {
        await page.elementLocator(document.getElementById(id)).press('Enter');
    }
    expect(clicks).toEqual([true, true, true]);
    expect(document.getElementById('summaryInput').getAttribute('tabindex')).toBe('0');
    const label = document.getElementById('summaryLabel');
    label.focus();
    key(label, 'keydown', ' ');
    key(label, 'keyup', ' ');
    expect(document.getElementById('summaryCheckbox').checked).toBe(true);

    const movedIcon = document.createElement('div');
    movedIcon.className = 'inline-drawer-icon';
    document.body.append(movedIcon);
    await nextTask();
    expect(movedIcon.getAttribute('tabindex')).toBe('0');
    document.getElementById('native').append(movedIcon);
    await nextTask();
    expect(movedIcon.hasAttribute('role')).toBe(false);
    expect(movedIcon.hasAttribute('tabindex')).toBe(false);
});

test('Space commits once on release and cancellation, composition, modifiers or lost focus do not activate', async () => {
    await render('<div id="action" class="menu_button">Action</div><input id="other">');
    const action = document.getElementById('action');
    const other = document.getElementById('other');
    let clicks = 0;
    action.addEventListener('click', () => clicks++);
    action.focus();
    expect(key(action, 'keydown', ' ').defaultPrevented).toBe(true);
    key(action, 'keydown', ' ', { repeat: true });
    expect(clicks).toBe(0);
    key(action, 'keyup', ' ');
    key(action, 'keyup', ' ');
    expect(clicks).toBe(1);
    key(action, 'keydown', ' ');
    other.focus();
    action.focus();
    key(action, 'keyup', ' ');
    expect(clicks).toBe(1);
    for (const options of [{ isComposing: true }, { altKey: true }, { ctrlKey: true }, { shiftKey: true }, { metaKey: true }]) {
        key(action, 'keydown', 'Enter', options);
        key(action, 'keydown', ' ', options);
        key(action, 'keyup', ' ', options);
    }
    action.addEventListener('keydown', event => event.preventDefault());
    key(action, 'keydown', 'Enter');
    key(action, 'keydown', ' ');
    key(action, 'keyup', ' ');
    expect(clicks).toBe(1);
});

test('business ancestor disabled state leaves child actions reachable while control state blocks activation', async () => {
    await render(`
        <div id="group" class="group_member disabled"><div id="enable" class="right_menu_button">Enable</div></div>
        <div id="disabled" class="menu_button disabled">Disabled</div>
        <div id="aria" class="menu_button" aria-disabled="true">Unavailable</div>
        <button id="native" disabled>Unavailable</button>
    `);
    const enable = document.getElementById('enable');
    let clicks = 0;
    for (const id of ['enable', 'disabled', 'aria', 'native']) {
        const control = document.getElementById(id);
        control.addEventListener('click', () => clicks++);
        key(control, 'keydown', 'Enter');
    }
    expect(enable.getAttribute('tabindex')).toBe('0');
    expect(clicks).toBe(1);
    enable.focus();
    key(enable, 'keydown', ' ');
    enable.setAttribute('aria-disabled', 'true');
    key(enable, 'keyup', ' ');
    await nextTask();
    expect(clicks).toBe(1);
    expect(enable.hasAttribute('tabindex')).toBe(false);
    enable.removeAttribute('aria-disabled');
    await nextTask();
    expect(enable.getAttribute('tabindex')).toBe('0');

    const group = document.getElementById('group');
    group.classList.add('not_focusable');
    await nextTask();
    expect(enable.hasAttribute('tabindex')).toBe(false);
    group.classList.remove('not_focusable');
    await nextTask();
    expect(enable.getAttribute('tabindex')).toBe('0');
});

test('label proxies retain checkbox, file and submit activation without taking over label semantics', async () => {
    await render(`
        <label id="checkboxLabel" class="menu_button" for="checkbox">Toggle</label><input id="checkbox" type="checkbox" hidden>
        <label id="fileLabel" class="menu_button" for="file">Upload</label><input id="file" type="file" hidden>
        <form><label id="submitLabel" class="menu_button" for="submit">Submit</label><input id="submit" type="submit" hidden></form>
    `);
    document.querySelector('form').addEventListener('submit', event => event.preventDefault());
    for (const kind of ['checkbox', 'file', 'submit']) {
        const label = document.getElementById(`${kind}Label`);
        const control = document.getElementById(kind);
        let clicks = 0;
        control.addEventListener('click', () => clicks++);
        expect(label.hasAttribute('role')).toBe(false);
        expect(label.getAttribute('tabindex')).toBe('0');
        label.focus();
        key(label, 'keydown', 'Enter');
        key(label, 'keydown', ' ');
        key(label, 'keyup', ' ');
        expect(clicks).toBe(2);
        control.disabled = true;
        key(label, 'keydown', 'Enter');
        expect(clicks).toBe(2);
        await nextTask();
        expect(label.hasAttribute('tabindex')).toBe(false);
    }
});

test('registered extension actions are discovered dynamically and native transitions stop synthetic activation', async () => {
    await render('<div id="extension" class="extension-action">Extension</div><a id="changing" class="menu_button"><i id="changingIcon" class="menu_button">Action</i></a>');
    registerInteractableType('.extension-action');
    const extension = document.getElementById('extension');
    const later = document.createElement('div');
    later.className = 'extension-action';
    document.body.append(later);
    await nextTask();
    for (const action of [extension, later]) {
        let clicks = 0;
        action.addEventListener('click', () => clicks++);
        action.focus();
        expect(document.activeElement).toBe(action);
        key(action, 'keydown', 'Enter');
        expect(clicks).toBe(1);
    }
    const changing = document.getElementById('changing');
    changing.setAttribute('href', '#destination');
    await nextTask();
    expect(changing.hasAttribute('role')).toBe(false);
    expect(changing.hasAttribute('tabindex')).toBe(false);
    expect(document.getElementById('changingIcon').hasAttribute('role')).toBe(false);
    expect(document.getElementById('changingIcon').hasAttribute('tabindex')).toBe(false);
    let clicks = 0;
    changing.addEventListener('click', () => clicks++);
    key(changing, 'keydown', 'Enter');
    expect(clicks).toBe(0);
});
