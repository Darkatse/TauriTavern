import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

/** Serves `prompts` to successive prompt reads until one is answered, and records the answers. */
function stubSafeMode(window, prompts) {
    const answers = [];
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (command, ...args) => {
        switch (command) {
            case 'get_safe_mode_prompt':
                return prompts.shift() ?? null;
            case 'dismiss_safe_mode_prompt':
            case 'request_safe_mode':
                answers.push(command);
                prompts = [];
                return undefined;
            default:
                return invoke(command, ...args);
        }
    };
    return answers;
}

function shownDialog(window, id) {
    return new Promise(resolve => {
        const observer = new window.MutationObserver(() => {
            const host = window.document.getElementById(id);
            if (host) {
                observer.disconnect();
                resolve(host.shadowRoot);
            }
        });
        observer.observe(window.document.body, { childList: true });
    });
}

async function startApp({ window, fixture, load, getModule, startHost }) {
    const requests = [];
    const handle = fixture.backend.handle.bind(fixture.backend);
    fixture.backend.handle = request => {
        requests.push(request.url);
        return handle(request);
    };
    await startHost();
    await load('script.js');
    const { eventSource, event_types } = getModule('scripts/events.js').namespace;
    const ready = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
    return {
        ready,
        settingsRead: () => requests.includes('/api/settings/get'),
        userStylesheet: () => window.document.querySelector('link[href="css/user.css"]'),
        begin: () => window.jQuery.holdReady(false),
    };
}

test('the safe mode offer comes before the backend is ready, and a normal start goes on', async context => {
    const runtime = await createBrowserRuntime();
    const { window } = runtime;
    context.after(() => window.happyDOM.close());
    const answers = stubSafeMode(window, [{ kind: 'offer' }]);
    const app = await startApp(runtime);
    const backendReady = window.__TAURITAVERN_MAIN_READY__;
    const backend = Promise.withResolvers();
    window.__TAURITAVERN_MAIN_READY__ = backend.promise;

    const offer = shownDialog(window, 'tt-safe-mode');
    app.begin();
    const dialog = await offer;
    assert.equal(app.userStylesheet(), null);

    dialog.querySelector('button[name="normal"]').click();
    backend.resolve(backendReady);
    await app.ready;
    assert.deepEqual(answers, ['dismiss_safe_mode_prompt']);
    assert.ok(app.settingsRead());
    assert.notEqual(app.userStylesheet(), null);
});

test('after a rescue, settings and user styles wait for the safe mode report', async context => {
    const runtime = await createBrowserRuntime();
    const { window } = runtime;
    context.after(() => window.happyDOM.close());
    const report = { kind: 'report', autoUploadPaused: true };
    // A rescue launch: the report exists once backend initialization has performed the rescue.
    const answers = stubSafeMode(window, [null, report]);
    const app = await startApp(runtime);

    const shown = shownDialog(window, 'tt-safe-mode');
    app.begin();
    const dialog = await shown;
    assert.equal(app.settingsRead(), false);
    assert.equal(app.userStylesheet(), null);

    dialog.querySelector('button[name="continue"]').click();
    await app.ready;
    assert.deepEqual(answers, ['dismiss_safe_mode_prompt']);
    assert.ok(app.settingsRead());
    assert.notEqual(app.userStylesheet(), null);
});

test('the safe mode offer enters safe mode after ten visible seconds', async context => {
    const { window, load } = await createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    const answers = stubSafeMode(window, [{ kind: 'offer' }]);
    const { settleSafeModePrompt } = (await load('scripts/tauri/startup/safe-mode.js')).namespace;
    let visibility = 'hidden';
    Object.defineProperty(window.document, 'visibilityState', { configurable: true, get: () => visibility });
    const timers = [];
    window.setTimeout = callback => timers.push(callback);
    window.clearTimeout = () => {};
    const elapse = seconds => {
        for (let second = 0; second < seconds; second++) timers.shift()();
    };

    const offer = shownDialog(window, 'tt-safe-mode');
    void settleSafeModePrompt();
    await offer;
    elapse(30);
    visibility = 'visible';
    elapse(9);
    assert.deepEqual(answers, [], 'Hidden time does not count');

    elapse(1);
    assert.deepEqual(answers, ['request_safe_mode']);
});
