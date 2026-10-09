import assert from 'node:assert/strict';
import test from 'node:test';
import { computeAccessibleName } from 'dom-accessibility-api';
import { createBrowserRuntime } from './runtime.mjs';

test('Popup naming and result controls', async context => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    try {
        await startHost();
        await load('script.js');
        const { Popup, POPUP_TYPE, POPUP_RESULT } = getModule('scripts/popup.js').namespace;
        const { power_user, send_on_enter_options } = getModule('scripts/power-user.js').namespace;
        getModule('scripts/keyboard.js').namespace.initKeyboard();
        const { document } = window;
        power_user.send_on_enter = send_on_enter_options.ENABLED;

        await context.test('expanded editors write through native and jQuery input handlers', async () => {
            getModule('scripts/chats.js').namespace.initChatUtilities();
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
                assert.deepEqual(values, { native: 'Edited greeting', jquery: 'Edited greeting' });
            } finally {
                await popup.completeAffirmative();
                await click.result;
                fixture.remove();
            }
        });

        await context.test('extension menus let Tab leave, consume Escape and hand dialogs a visible focus origin', async () => {
            await getModule('scripts/extensions.js').namespace.ensureExtensionsUiReady();
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
            assert.equal(document.activeElement, panel);
            const input = document.getElementById('send_textarea');
            input.focus();
            assert.equal(trigger.getAttribute('aria-expanded'), 'true', 'focus may leave without dismissing the menu');
            action.focus();
            let escapedToDocument = false;
            const onEscape = () => { escapedToDocument = true; };
            document.addEventListener('keydown', onEscape);
            const escape = new window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
            action.dispatchEvent(escape);
            document.removeEventListener('keydown', onEscape);
            assert.equal(escape.defaultPrevented, true);
            assert.equal(escapedToDocument, false);
            assert.equal(document.activeElement, trigger);

            trigger.click();
            action.focus();
            action.click();
            const popup = Popup.util.popups.at(-1);
            assert.equal(panel.style.display, 'none');
            // Native dialog autofocus/restoration is verified in WebView.
            assert.equal(focusAtOpen, trigger);
            assert.equal(popup.dlg.open, true);
            await popup.completeCancelled();
            await shown;

            trigger.click();
            input.focus();
            input.click();
            assert.equal(panel.style.display, 'none');
            assert.equal(document.activeElement, input, 'outside clicks keep their own focus');
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

        await context.test('simultaneous dialogs keep their own accessible name when a title changes', async () => {
            const pending = [
                Popup.show.input('Rename <em>device</em>', 'Input help', '', { animation: 'none' }),
                Popup.show.confirm('Confirm change', '<h2>Body heading</h2>', { animation: 'none' }),
            ];
            const popups = Array.from(Popup.util.popups);
            assert.deepEqual(popups.map(popup => computeAccessibleName(popup.dlg)), ['Rename device', 'Confirm change']);
            popups[0].content.firstElementChild.textContent = '重命名设备';
            assert.deepEqual(popups.map(popup => computeAccessibleName(popup.dlg)), ['重命名设备', 'Confirm change']);
            for (const popup of popups.toReversed()) await popup.completeCancelled();
            await Promise.all(pending);
        });

        await context.test('action-only and cancelled buttons use click without coercing their results', async () => {
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
            // happy-dom cannot synthesize a browser's default activation. Popup must leave it alone.
            assert.equal(key.defaultPrevented, false);
            assert.equal(popup.result, undefined);
            preview.click();
            assert.equal(actions, 1);
            assert.equal(popup.dlg.open, true);
            assert.equal(popup.result, undefined);
            back.click();
            assert.equal(await shown, null);

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
            assert.equal(await numericShown, 1001);
            assert.equal(actionFinished, false, 'automatic completion does not start awaiting custom actions');
            actionGate.resolve();
            await actionDone.promise;
        });

        await context.test('extension disabled state blocks Popup actions and input submission while allowing recovery', async () => {
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
            assert.equal(actions, 0);
            assert.equal(popup.dlg.open, true);

            let independentActions = 0;
            const independent = popup.content.querySelector('button');
            independent.addEventListener('click', () => independentActions++);
            independent.click();
            assert.equal(independentActions, 1);

            preview.classList.remove('disabled');
            preview.click();
            assert.equal(actions, 1);
            await popup.completeAffirmative();
            assert.equal(await shown, 'value');
        });

        await context.test('dynamic async close rejection keeps the original result and allows retry', async () => {
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
            assert.deepEqual(results, [POPUP_RESULT.AFFIRMATIVE]);
            assert.equal(popup.result, POPUP_RESULT.AFFIRMATIVE);
            validation.resolve(false);
            assert.equal(await attempt, undefined);
            assert.equal(popup.result, undefined);
            assert.equal(popup.dlg.open, true);
            popup.onClosing = current => current.result === POPUP_RESULT.CANCELLED;
            popup.dlg.dispatchEvent(new window.Event('cancel', { cancelable: true }));
            assert.equal(await shown, null);
        });

        await context.test('input Enter respects editing rules and legacy result actions retain their click', async () => {
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
            assert.deepEqual(results, []);
            await enter(popup.mainInput, { ctrlKey: true });
            assert.deepEqual(results, [1]);
            power_user.send_on_enter = send_on_enter_options.DISABLED;
            await enter(popup.mainInput, { ctrlKey: true });
            assert.deepEqual(results, [1]);
            power_user.send_on_enter = send_on_enter_options.ENABLED;
            popup.mainInput.rows = 1;
            popup.mainInput.dataset.result = 'undefined';
            await enter(popup.mainInput);
            assert.deepEqual(results, [1]);
            popup.mainInput.dataset.result = 'null';
            await enter(popup.mainInput);
            assert.deepEqual(results, [1, null]);
            popup.mainInput.classList.remove('result-control');
            await enter(popup.mainInput);
            assert.deepEqual(results, [1, null]);
            await enter(legacy);
            assert.equal(actions, 1);
            assert.deepEqual(results, [1, null, 1001]);
            popup.onClosing = null;
            await popup.completeCancelled();
            await shown;
        });
    } finally {
        await window.happyDOM.close();
    }
});
