import test from 'node:test';
import assert from 'node:assert/strict';
import { Window } from 'happy-dom';
import { installLayoutApi } from '../src/tauri/main/api/layout.js';

test('layout reports the usable viewport and publishes browser resizes', async () => {
    const window = new Window({ width: 390, height: 844 });
    globalThis.window = window;
    globalThis.requestAnimationFrame = window.requestAnimationFrame.bind(window);
    window.__TAURITAVERN__ = { api: {} };
    installLayoutApi();
    const layout = window.__TAURITAVERN__.api.layout;

    let received;
    const resized = new Promise(resolve => { received = resolve; });
    const unsubscribe = await layout.subscribe(snapshot => {
        if (snapshot.viewport.height === 540) received(snapshot);
    });
    window.happyDOM.setWindowSize({ width: 390, height: 540 });
    window.dispatchEvent(new window.Event('resize'));
    assert.equal((await resized).safeFrame.height, 540);
    await unsubscribe();
    await window.happyDOM.close();
    delete globalThis.window;
    delete globalThis.requestAnimationFrame;
});
