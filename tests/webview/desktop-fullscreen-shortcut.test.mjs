import { expect, rs, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const {
    installDesktopFullscreenShortcut,
    leaveDesktopFullscreenForShutdown,
} = await importFrontend('tauri/main/adapters/window/desktop-fullscreen-shortcut.js');

const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));
const keydown = init => new KeyboardEvent('keydown', { key: 'F11', cancelable: true, ...init });

test('F11 toggles native fullscreen and restores windowed geometry before shutdown', async () => {
    let fullscreen = false;
    let resizeListener = null;
    let moveListener = null;
    const requestedStates = [];
    const windowedSize = { width: 1280, height: 800 };
    const windowedPosition = { x: 320, y: 109 };
    rs.stubGlobal('__TAURI__', {
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
    });

    installDesktopFullscreenShortcut(window);
    installDesktopFullscreenShortcut(window);

    expect(document.dispatchEvent(keydown())).toBe(false);
    await nextTask();
    expect(requestedStates).toEqual([true]);

    document.dispatchEvent(keydown());
    document.dispatchEvent(keydown());
    await nextTask();
    expect(requestedStates).toEqual([true, false, true]);

    const repeat = keydown({ repeat: true });
    expect(document.dispatchEvent(repeat)).toBe(false);
    expect(repeat.defaultPrevented).toBe(true);

    const modified = keydown({ ctrlKey: true });
    expect(document.dispatchEvent(modified)).toBe(true);
    expect(modified.defaultPrevented).toBe(false);
    expect(requestedStates).toEqual([true, false, true]);

    await leaveDesktopFullscreenForShutdown('visibilitychange:hidden');
    expect(fullscreen).toBe(true);
    await leaveDesktopFullscreenForShutdown('tauri:exit-requested');
    expect(fullscreen).toBe(false);
    expect(resizeListener).toBeNull();
    expect(moveListener).toBeNull();
    expect(requestedStates).toEqual([true, false, true, false]);
});
