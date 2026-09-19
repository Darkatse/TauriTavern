// pnpm run check:startup (also included in pnpm run check).
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
} finally {
    await window.happyDOM.close();
}
