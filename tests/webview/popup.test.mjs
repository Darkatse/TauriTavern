import { expect, test } from '@rstest/core';
import { computeAccessibleName } from 'dom-accessibility-api';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost, close } = await createFrontend();
await startHost();
await load('script.js');
const { Popup, POPUP_TYPE, POPUP_RESULT } = (await load('scripts/popup.js'));
const { power_user, send_on_enter_options } = (await load('scripts/power-user.js'));
(await load('scripts/keyboard.js')).initKeyboard();
const { document } = window;
power_user.send_on_enter = send_on_enter_options.ENABLED;

test('expanded editors write through native and jQuery input handlers', async () => {
    (await load('scripts/chats.js')).initChatUtilities();
    const fixture = document.createElement('div');
    fixture.innerHTML = '<div id="expanded-editor-source" contenteditable="true"></div><button type="button" class="editor_maximize" data-for="expanded-editor-source"></button>';
    document.body.append(fixture);
    const source = fixture.querySelector('[contenteditable]');
    const values = {};
    source.addEventListener('input', () => { values.native = source.innerText; });
    window.jQuery(source).on('input', () => { values.jquery = source.innerText; });
    const click = window.jQuery.Event('click');
    window.jQuery(fixture.querySelector('button')).trigger(click);
    const popup = Popup.util.popups.at(-1);
    try {
        const editor = popup.dlg.querySelector('textarea.maximized_textarea');
        editor.value = 'Edited greeting';
        editor.dispatchEvent(new window.Event('input', { bubbles: true }));
        expect(values).toEqual({ native: 'Edited greeting', jquery: 'Edited greeting' });
    } finally {
        await popup.completeAffirmative();
        await click.result;
        fixture.remove();
    }
});

test('extension menus let Tab leave, consume Escape and hand dialogs a visible focus origin', async () => {
    await (await load('scripts/extensions.js')).ensureExtensionsUiReady();
    const trigger = document.getElementById('extensionsMenuButton');
    const panel = document.getElementById('extensionsMenu');
    const action = document.createElement('button');
    action.textContent = 'Edit extension';
    panel.append(action);
    let shown;
    let focusAtOpen;
    action.addEventListener('click', () => {
        focusAtOpen = document.activeElement;
        shown = Popup.show.input('Extension setting', '', '', { animation: 'none' });
    });

    trigger.click();
    expect(document.activeElement).toBe(panel);
    const input = document.getElementById('send_textarea');
    input.focus();
    expect(trigger.getAttribute('aria-expanded'), 'focus may leave without dismissing the menu').toBe('true');
    action.focus();
    let escapedToDocument = false;
    const onEscape = () => { escapedToDocument = true; };
    document.addEventListener('keydown', onEscape);
    const escape = new window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    action.dispatchEvent(escape);
    document.removeEventListener('keydown', onEscape);
    expect(escape.defaultPrevented).toBe(true);
    expect(escapedToDocument).toBe(false);
    expect(document.activeElement).toBe(trigger);

    trigger.click();
    action.focus();
    action.click();
    const popup = Popup.util.popups.at(-1);
    expect(panel.style.display).toBe('none');
    // Native dialog autofocus/restoration is verified in WebView.
    expect(focusAtOpen).toBe(trigger);
    expect(popup.dlg.open).toBe(true);
    await popup.completeCancelled();
    await shown;

    trigger.click();
    input.focus();
    input.click();
    expect(panel.style.display).toBe('none');
    expect(document.activeElement, 'outside clicks keep their own focus').toBe(input);
});

async function enter(control, options = {}, cancelled = false) {
    control.focus();
    const event = new window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, ...options });
    if (cancelled) event.preventDefault();
    control.dispatchEvent(event);
    // A synchronous onClosing rejection resumes complete() in this microtask.
    await Promise.resolve();
    return event;
}

test('simultaneous dialogs keep their own accessible name when a title changes', async () => {
    const pending = [
        Popup.show.input('Rename <em>device</em>', 'Input help', '', { animation: 'none' }),
        Popup.show.confirm('Confirm change', '<h2>Body heading</h2>', { animation: 'none' }),
    ];
    const popups = Array.from(Popup.util.popups);
    expect(popups.map(popup => computeAccessibleName(popup.dlg))).toEqual(['Rename device', 'Confirm change']);
    popups[0].content.firstElementChild.textContent = '重命名设备';
    expect(popups.map(popup => computeAccessibleName(popup.dlg))).toEqual(['重命名设备', 'Confirm change']);
    for (const popup of popups.toReversed()) await popup.completeCancelled();
    await Promise.all(pending);
});

test('action-only and cancelled buttons use click without coercing their results', async () => {
    let actions = 0;
    const popup = new Popup('Actions', POPUP_TYPE.TEXT, '', {
        animation: 'none',
        customButtons: [
            { text: 'Preview', action: () => actions++ },
            { text: 'Go back', result: null },
        ],
    });
    const shown = popup.show();
    const [preview, back] = popup.buttonControls.querySelectorAll('.popup-button-custom');
    const key = await enter(preview);
    // A synthetic key gets no default activation; Popup must leave that activation to the browser.
    expect(key.defaultPrevented).toBe(false);
    expect(popup.result).toBeUndefined();
    preview.click();
    expect(actions).toBe(1);
    expect(popup.dlg.open).toBe(true);
    expect(popup.result).toBeUndefined();
    back.click();
    expect(await shown).toBeNull();

    const actionGate = Promise.withResolvers();
    const actionDone = Promise.withResolvers();
    let actionFinished = false;
    const numeric = new Popup('Result', POPUP_TYPE.TEXT, '', {
        animation: 'none',
        customButtons: [{ text: 'Choose', result: 1001, action: async () => {
            await actionGate.promise;
            actionFinished = true;
            actionDone.resolve();
        } }],
    });
    const numericShown = numeric.show();
    numeric.buttonControls.querySelector('.popup-button-custom').click();
    expect(await numericShown).toBe(1001);
    expect(actionFinished, 'automatic completion does not start awaiting custom actions').toBe(false);
    actionGate.resolve();
    await actionDone.promise;
});

test('extension disabled state blocks Popup actions and input submission while allowing recovery', async () => {
    let actions = 0;
    const popup = new Popup('<div class="disabled"><button type="button">Independent action</button></div>', POPUP_TYPE.INPUT, 'value', {
        animation: 'none', customButtons: [{ text: 'Preview', action: () => actions++ }],
    });
    const shown = popup.show();
    const preview = popup.buttonControls.querySelector('.popup-button-custom');
    preview.classList.add('disabled');
    popup.okButton.setAttribute('aria-disabled', 'true');
    preview.click();
    await enter(popup.mainInput);
    expect(actions).toBe(0);
    expect(popup.dlg.open).toBe(true);

    let independentActions = 0;
    const independent = popup.content.querySelector('button');
    independent.addEventListener('click', () => independentActions++);
    independent.click();
    expect(independentActions).toBe(1);

    preview.classList.remove('disabled');
    preview.click();
    expect(actions).toBe(1);
    await popup.completeAffirmative();
    expect(await shown).toBe('value');
});

test('dynamic async close rejection keeps the original result and allows retry', async () => {
    const popup = new Popup('Validate', POPUP_TYPE.CONFIRM, '', { animation: 'none' });
    const shown = popup.show();
    const validation = Promise.withResolvers();
    const results = [];
    popup.onClosing = async current => {
        results.push(current.result);
        return await validation.promise;
    };
    const attempt = popup.completeAffirmative();
    popup.cancelButton.click();
    expect(results).toEqual([POPUP_RESULT.AFFIRMATIVE]);
    expect(popup.result).toBe(POPUP_RESULT.AFFIRMATIVE);
    validation.resolve(false);
    expect(await attempt).toBeUndefined();
    expect(popup.result).toBeUndefined();
    expect(popup.dlg.open).toBe(true);
    popup.onClosing = current => current.result === POPUP_RESULT.CANCELLED;
    popup.dlg.dispatchEvent(new window.Event('cancel', { cancelable: true }));
    expect(await shown).toBeNull();
});

test('input Enter respects editing rules and legacy result actions retain their click', async () => {
    const legacy = document.createElement('div');
    legacy.className = 'result-control menu_button';
    legacy.tabIndex = 0;
    legacy.dataset.result = '1001';
    let actions = 0;
    legacy.addEventListener('click', () => actions++);
    const popup = new Popup(legacy, POPUP_TYPE.INPUT, '', { rows: 2, animation: 'none' });
    const shown = popup.show();
    const results = [];
    popup.onClosing = current => { results.push(current.result); return false; };
    await enter(popup.mainInput);
    await enter(popup.mainInput, { ctrlKey: true, isComposing: true });
    await enter(popup.mainInput, { ctrlKey: true }, true);
    expect(results).toEqual([]);
    await enter(popup.mainInput, { ctrlKey: true });
    expect(results).toEqual([1]);
    power_user.send_on_enter = send_on_enter_options.DISABLED;
    await enter(popup.mainInput, { ctrlKey: true });
    expect(results).toEqual([1]);
    power_user.send_on_enter = send_on_enter_options.ENABLED;
    popup.mainInput.rows = 1;
    popup.mainInput.dataset.result = 'undefined';
    await enter(popup.mainInput);
    expect(results).toEqual([1]);
    popup.mainInput.dataset.result = 'null';
    await enter(popup.mainInput);
    expect(results).toEqual([1, null]);
    popup.mainInput.classList.remove('result-control');
    await enter(popup.mainInput);
    expect(results).toEqual([1, null]);
    await enter(legacy);
    expect(actions).toBe(1);
    expect(results).toEqual([1, null, 1001]);
    popup.onClosing = null;
    await popup.completeCancelled();
    await shown;
});
