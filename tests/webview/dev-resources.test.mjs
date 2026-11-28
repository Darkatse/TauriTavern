import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

function injectBootstrapScript(window, devServerOrigin) {
    // Modules have no currentScript; stand in for the injected tag.
    const script = window.document.createElement('script');
    script.dataset.devServerOrigin = devServerOrigin;
    Object.defineProperty(window.document, 'currentScript', { configurable: true, value: script });
}

test('dev READY resolves only after the active worker controls the page', async () => {
    const { window, load, close } = await createFrontend();
    const claimRequested = Promise.withResolvers();
    const allowControl = Promise.withResolvers();
    const serviceWorker = new window.EventTarget();
    let claimCompletion;
    const active = {
        postMessage(data) {
            const event = new window.Event('message');
            Object.assign(event, {
                data,
                waitUntil: promise => { claimCompletion = promise; },
            });
            window.dispatchEvent(event);
        },
    };
    const registration = { active };
    serviceWorker.controller = null;
    serviceWorker.ready = Promise.resolve(registration);
    serviceWorker.register = async () => registration;
    Object.defineProperty(window.navigator, 'serviceWorker', { value: serviceWorker });
    window.clients = {
        async claim() {
            claimRequested.resolve();
            await allowControl.promise;
            serviceWorker.controller = active;
            serviceWorker.dispatchEvent(new window.Event('controllerchange'));
        },
    };
    window.sessionStorage.setItem('tauritavern:dev-sw-session', 'active');
    window.__TAURI__.core.convertFileSrc = () => 'tt-ext://localhost/';
    injectBootstrapScript(window, window.location.origin);

    try {
        await load('tt-ext-sw.js');
        await load('dev-sw-bootstrap.js');
        let ready = false;
        window.__TAURITAVERN_DEV_SW_READY__.then(() => { ready = true; });
        await claimRequested.promise;
        expect(ready).toBe(false);
        allowControl.resolve();
        await window.__TAURITAVERN_DEV_SW_READY__;
        await claimCompletion;
        expect(ready).toBe(true);
    } finally {
        close();
    }
});

test('a document loaded through the app protocol does not install the dev bridge', async () => {
    const { window, load, close } = await createFrontend();
    Object.defineProperty(window.navigator, 'serviceWorker', {
        value: { register: () => expect.unreachable('the app protocol serves host resources natively') },
    });
    injectBootstrapScript(window, 'http://localhost:1430');

    try {
        await load('dev-sw-bootstrap.js');
        expect(window.__TAURITAVERN_DEV_SW_READY__).toBeUndefined();
    } finally {
        close();
    }
});
