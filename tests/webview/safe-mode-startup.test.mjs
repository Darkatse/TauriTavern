import { expect, onTestFinished, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

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

async function startApp({ window, fixture, load, startHost }) {
    const requests = [];
    const handle = fixture.backend.handle.bind(fixture.backend);
    fixture.backend.handle = request => {
        requests.push(request.url);
        return handle(request);
    };
    await startHost();
    await load('script.js');
    const { eventSource, event_types } = (await load('scripts/events.js'));
    const ready = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
    return {
        ready,
        settingsRead: () => requests.includes('/api/settings/get'),
        userStylesheet: () => window.document.querySelector('link[href="css/user.css"]'),
        begin: () => window.jQuery.holdReady(false),
    };
}

test('the safe mode offer comes before the backend is ready, and a normal start goes on', async () => {
    const frontend = await createFrontend();
    const { window, close } = frontend;
    onTestFinished(close);
    const answers = stubSafeMode(window, [{ kind: 'offer' }]);
    const app = await startApp(frontend);
    const backendReady = window.__TAURITAVERN_MAIN_READY__;
    const backend = Promise.withResolvers();
    window.__TAURITAVERN_MAIN_READY__ = backend.promise;

    const offer = shownDialog(window, 'tt-safe-mode');
    app.begin();
    const dialog = await offer;
    expect(app.userStylesheet()).toBeNull();

    dialog.querySelector('button[name="normal"]').click();
    backend.resolve(backendReady);
    await app.ready;
    expect(answers).toEqual(['dismiss_safe_mode_prompt']);
    expect(app.settingsRead()).toBeTruthy();
    expect(app.userStylesheet()).not.toBeNull();
});

test('after a rescue, settings and user styles wait for the safe mode report', async () => {
    const frontend = await createFrontend();
    const { window, close } = frontend;
    onTestFinished(close);
    const report = { kind: 'report', autoUploadPaused: true };
    // A rescue launch: the report exists once backend initialization has performed the rescue.
    const answers = stubSafeMode(window, [null, report]);
    const app = await startApp(frontend);

    const shown = shownDialog(window, 'tt-safe-mode');
    app.begin();
    const dialog = await shown;
    expect(app.settingsRead()).toBe(false);
    expect(app.userStylesheet()).toBeNull();

    dialog.querySelector('button[name="continue"]').click();
    await app.ready;
    expect(answers).toEqual(['dismiss_safe_mode_prompt']);
    expect(app.settingsRead()).toBeTruthy();
    expect(app.userStylesheet()).not.toBeNull();
});

test('the safe mode offer enters safe mode after ten visible seconds', async () => {
    const { window, load } = await createFrontend();
    onTestFinished(close);
    const answers = stubSafeMode(window, [{ kind: 'offer' }]);
    const { settleSafeModePrompt } = (await load('scripts/tauri/startup/safe-mode.js'));
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
    expect(answers, 'Hidden time does not count').toEqual([]);

    elapse(1);
    expect(answers).toEqual(['request_safe_mode']);
});
