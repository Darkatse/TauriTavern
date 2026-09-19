import assert from 'node:assert/strict';
import test from 'node:test';
import { Window } from 'happy-dom';

import {
    installDesktopFullscreenShortcut,
    leaveDesktopFullscreenForShutdown,
} from '../src/tauri/main/adapters/window/desktop-fullscreen-shortcut.js';

const flushTasks = () => new Promise(resolve => setImmediate(resolve));

test('F11 toggles native fullscreen and restores windowed geometry before shutdown', async () => {
    const window = new Window({ url: 'https://tauritavern.local/' });
    let fullscreen = false;
    let resizeListener = null;
    let moveListener = null;
    const requestedStates = [];
    const windowedSize = { width: 1280, height: 800 };
    const windowedPosition = { x: 320, y: 109 };
    window.__TAURI__ = {
        window: {
            getCurrentWindow: () => ({
                isFullscreen: async () => fullscreen,
                innerSize: async () => fullscreen ? { width: 1920, height: 1080 } : windowedSize,
                outerPosition: async () => fullscreen ? { x: 0, y: 0 } : windowedPosition,
                setFullscreen: async (value) => {
                    fullscreen = value;
                    requestedStates.push(value);
                    resizeListener?.();
                    moveListener?.();
                },
                onResized: async (listener) => {
                    resizeListener = listener;
                    return () => { resizeListener = null; };
                },
                onMoved: async (listener) => {
                    moveListener = listener;
                    return () => { moveListener = null; };
                },
            }),
        },
    };
    globalThis.window = window;

    try {
        installDesktopFullscreenShortcut(window);
        installDesktopFullscreenShortcut(window);

        const enterFullscreen = new window.KeyboardEvent('keydown', {
            key: 'F11',
            cancelable: true,
        });
        assert.equal(window.document.dispatchEvent(enterFullscreen), false);
        await flushTasks();
        assert.deepEqual(requestedStates, [true]);

        window.document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'F11', cancelable: true }));
        window.document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'F11', cancelable: true }));
        await flushTasks();
        assert.deepEqual(requestedStates, [true, false, true]);

        const repeat = new window.KeyboardEvent('keydown', {
            key: 'F11',
            repeat: true,
            cancelable: true,
        });
        assert.equal(window.document.dispatchEvent(repeat), false);
        assert.equal(repeat.defaultPrevented, true);

        const modified = new window.KeyboardEvent('keydown', {
            key: 'F11',
            ctrlKey: true,
            cancelable: true,
        });
        assert.equal(window.document.dispatchEvent(modified), true);
        assert.equal(modified.defaultPrevented, false);
        assert.deepEqual(requestedStates, [true, false, true]);

        await leaveDesktopFullscreenForShutdown('visibilitychange:hidden');
        assert.equal(fullscreen, true);
        await leaveDesktopFullscreenForShutdown('tauri:exit-requested');
        assert.equal(fullscreen, false);
        assert.equal(resizeListener, null);
        assert.equal(moveListener, null);
        assert.deepEqual(requestedStates, [true, false, true, false]);
    } finally {
        delete globalThis.window;
        window.close();
    }
});
