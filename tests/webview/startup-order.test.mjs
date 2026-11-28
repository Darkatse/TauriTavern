import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, host, load, startHost } = await createFrontend();
const toasts = [];
const toastShown = Promise.withResolvers();
window.toastr.warning = message => { toasts.push(message); toastShown.resolve(); };
// Host-ready modules install their listeners after startup; emit once the sync listener exists.
const syncListening = Promise.withResolvers();
const listen = window.__TAURI__.event.listen;
window.__TAURI__.event.listen = (name, callback) => {
    if (name === 'sync_auto:toast') syncListening.resolve();
    return listen(name, callback);
};

test('main application initializes and early host events wait for APP_READY', async () => {
    await startHost();
    await syncListening.promise;
    host.emit('sync_auto:toast', { level: 'warning', message: 'Early sync event' });
    expect(toasts).toEqual([]);
    await load('script.js');
    expect(toasts, 'Module evaluation alone must not present application UI').toEqual([]);
    const { eventSource, event_types } = await load('scripts/events.js');
    await eventSource.emit(event_types.APP_READY);
    await toastShown.promise;
    expect(toasts, 'An event received before APP_READY must still be presented').toEqual(['Early sync event']);
});

test('a failed startup shows its error and still reports it', async () => {
    // Keep the failure observable through the real startup path.
    window.fetch = () => window.Promise.reject(new window.Error('Locale fixture unavailable'));
    const failure = new Promise(resolve => window.addEventListener('error', resolve, { once: true }));
    window.jQuery.holdReady(false);
    const { error } = await failure;
    const dialog = window.document.getElementById('tt-startup-failure').shadowRoot.querySelector('dialog');
    expect(dialog.open, 'A failed startup must stay visible').toBe(true);
    expect(dialog.querySelector('pre').textContent, 'The dialog must show the startup error').toContain(error.message);
});

test('character actions preserve the extension change contract', async () => {
    // The ready callback still binds DOM actions after settings fail; jQuery event.result exposes the core handler's promise.
    const dropdown = window.document.getElementById('char-management-dropdown');
    dropdown.add(new window.Option('Extension action', 'extension-action'));
    dropdown.lastElementChild.id = 'extension-action';
    let pendingAction;
    window.jQuery(dropdown).on('change', event => {
        expect(dropdown.selectedOptions[0].id, 'Synchronous extension handlers see their selected option').toBe('extension-action');
        pendingAction = event.result;
    });
    dropdown.value = 'extension-action';
    window.jQuery(dropdown).trigger('change');
    await pendingAction;
    expect(dropdown.selectedIndex, 'The completed action resets the menu').toBe(0);
});
