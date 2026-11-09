import { expect, onTestFinished, test } from '@rstest/core';
import { captureBrowserErrors, createFrontend } from '../helpers/frontend.mjs';

async function openChat() {
    const { window, host, fixture, load, startHost, close } = await createFrontend();
    onTestFinished(close);
    const { errors, failed } = captureBrowserErrors(window);
    await startHost();
    const main = await load('script.js');
    const { eventSource, event_types } = await load('scripts/events.js');
    const started = [event_types.APP_READY, event_types.SETTINGS_UPDATED, event_types.EXTENSION_SETTINGS_LOADED]
        .map(event => new Promise(resolve => eventSource.once(event, resolve)));
    window.jQuery.holdReady(false);
    await Promise.race([Promise.all(started), failed]);
    const message = window.document.querySelector('#chat .mes[mesid="0"]');
    const saved = () => JSON.parse(fixture.chatFiles.get('review-chat').split('\n')[1]).mes;
    return { window, host, main, eventSource, event_types, errors, failed, message, saved };
}

test('content updates leave an open message editor, and Done saves its draft', async () => {
    const { window, host, main, eventSource, event_types, errors, failed, message, saved } = await openChat();
    await main.messageEdit(0);
    const editor = message.querySelector('#curEditTextarea');
    editor.value = 'Edited';

    main.chat[0].mes = 'Changed by an extension';
    main.updateMessageBlock(0, main.chat[0]);
    await main.finalizeMessageContent(0);
    await main.refreshChatContent();
    expect(message.querySelector('#curEditTextarea')).toBe(editor);

    const updated = new Promise(resolve => eventSource.once(event_types.MESSAGE_UPDATED, resolve));
    window.jQuery(message).find('.mes_edit_done').trigger('click');
    await Promise.race([updated, failed]);
    expect(message.querySelector('#curEditTextarea')).toBeNull();
    expect(message.querySelector('.mes_text').textContent.trim()).toBe('Edited');
    await Promise.race([host.exit(), failed]);
    expect(saved()).toBe('Edited');
    expect(errors).toEqual([]);
});

test('an edit session ends with its editor, and Done finishes the session it started', async () => {
    const { window, host, main, eventSource, event_types, errors, failed, message, saved } = await openChat();
    const click = selector => window.jQuery(message).find(selector).trigger('click');
    const editor = () => message.querySelector('#curEditTextarea');
    const once = event => new Promise(resolve => eventSource.once(event, resolve));

    // Reopening while Cancel still awaits MESSAGE_UPDATED.
    click('.mes_edit');
    const cancelled = once(event_types.MESSAGE_UPDATED);
    click('.mes_edit_cancel');
    click('.mes_edit');
    expect(editor()).not.toBeNull();
    await Promise.race([cancelled, failed]);

    // Closing the editor while Done awaits a MESSAGE_EDITED listener.
    const held = Promise.withResolvers();
    const listener = () => held.promise;
    eventSource.on(event_types.MESSAGE_EDITED, listener);
    editor().value = 'Edited';
    click('.mes_edit_done');
    const closed = once(event_types.MESSAGE_UPDATED);
    main.closeMessageEditor();
    await Promise.race([closed, failed]);
    eventSource.removeListener(event_types.MESSAGE_EDITED, listener);
    const done = once(event_types.MESSAGE_UPDATED);
    held.resolve();
    await Promise.race([done, failed]);
    await Promise.race([host.exit(), failed]);
    expect(saved()).toBe('Edited');
    expect(errors).toEqual([]);
});
