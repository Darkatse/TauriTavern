import assert from 'node:assert/strict';
import { createBrowserRuntime } from './runtime.mjs';

const { window, listeners, getModule, load, startHost } = createBrowserRuntime();
const toasts = [];
window.toastr.warning = message => toasts.push(message);

try {
    await startHost();
    assert.ok(listeners.has('lan_sync:pair_request'), 'Pairing requests must be received before APP_READY');
    assert.ok(listeners.has('sync_auto:toast'), 'Sync events must be received before APP_READY');
    const earlyToast = listeners.get('sync_auto:toast')({ payload: { level: 'warning', message: 'Early sync event' } });
    assert.deepEqual(toasts, []);
    await assert.doesNotReject(() => load('script.js'),
        'A fast backend must not break main application module initialization');
    assert.deepEqual(toasts, [], 'Module evaluation alone must not present application UI');
    const { eventSource, event_types } = getModule('scripts/events.js').namespace;
    await eventSource.emit(event_types.APP_READY);
    await earlyToast;
    assert.deepEqual(toasts, ['Early sync event'], 'An event received before APP_READY must still be presented');
    console.log('PASS: main application initializes and early host events wait for APP_READY');

    // The runtime rejects every network request, so startup stops while loading locales.
    const failure = new Promise(resolve => window.addEventListener('error', resolve, { once: true }));
    window.jQuery.holdReady(false);
    const { error } = await failure;
    const dialog = window.document.getElementById('tt-startup-failure').shadowRoot.querySelector('dialog');
    assert.equal(dialog.open, true, 'A failed startup must stay visible');
    assert.ok(dialog.querySelector('pre').textContent.includes(error.message), 'The dialog must show the startup error');
    console.log('PASS: a failed startup shows its error and still reports it');

    // The ready callback still binds DOM actions after settings fail; jQuery event.result exposes the core handler's promise.
    const dropdown = window.document.getElementById('char-management-dropdown');
    dropdown.add(new window.Option('Extension action', 'extension-action'));
    dropdown.lastElementChild.id = 'extension-action';
    let pendingAction;
    window.jQuery(dropdown).on('change', event => {
        assert.equal(dropdown.selectedOptions[0].id, 'extension-action', 'Synchronous extension handlers see their selected option');
        pendingAction = event.result;
    });
    dropdown.value = 'extension-action';
    window.jQuery(dropdown).trigger('change');
    await pendingAction;
    assert.equal(dropdown.selectedIndex, 0, 'The completed action resets the menu');
    console.log('PASS: character actions preserve the extension change contract');
} finally {
    await window.happyDOM.close();
}
