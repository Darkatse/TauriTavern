// @ts-check

const installedDocuments = new WeakSet();
let fullscreenToggle = Promise.resolve();
const FULLSCREEN_EXIT_TIMEOUT_MS = 5000;
/** @type {{ size: { width: number; height: number }; position: { x: number; y: number } } | null} */
let windowedGeometry = null;

function getCurrentAppWindow() {
    const tauriWindow = window.__TAURI__?.window;
    if (typeof tauriWindow?.getCurrentWindow !== 'function') {
        throw new Error('Desktop fullscreen requires the Tauri window API');
    }

    return tauriWindow.getCurrentWindow();
}

/**
 * Leaves native fullscreen before the window-state plugin records final geometry.
 *
 * @param {string} reason
 */
export async function leaveDesktopFullscreenForShutdown(reason) {
    if (reason !== 'tauri:exit-requested') {
        return;
    }

    const appWindow = getCurrentAppWindow();
    if (!await appWindow.isFullscreen()) {
        return;
    }

    if (!windowedGeometry) {
        await appWindow.setFullscreen(false);
        return;
    }
    const expectedGeometry = windowedGeometry;

    /** @type {() => void} */
    let resolveRestored = () => {};
    /** @type {(reason?: unknown) => void} */
    let rejectRestored = () => {};
    /** @type {Promise<void>} */
    const restored = new Promise((resolve, reject) => {
        resolveRestored = () => resolve();
        rejectRestored = reject;
    });
    const checkRestored = () => {
        void Promise.all([appWindow.innerSize(), appWindow.outerPosition()])
            .then(([size, position]) => {
                if (size.width === expectedGeometry.size.width
                    && size.height === expectedGeometry.size.height
                    && position.x === expectedGeometry.position.x
                    && position.y === expectedGeometry.position.y) {
                    resolveRestored();
                }
            })
            .catch(rejectRestored);
    };
    const unlisten = await Promise.all([
        appWindow.onResized(checkRestored),
        appWindow.onMoved(checkRestored),
    ]);
    /** @type {number | undefined} */
    let timeoutId;
    const timeout = new Promise((_, reject) => {
        timeoutId = window.setTimeout(() => {
            reject(new Error('Timed out while leaving desktop fullscreen'));
        }, FULLSCREEN_EXIT_TIMEOUT_MS);
    });

    try {
        await appWindow.setFullscreen(false);
        checkRestored();
        await Promise.race([restored, timeout]);
    } finally {
        if (timeoutId !== undefined) {
            window.clearTimeout(timeoutId);
        }
        for (const removeListener of unlisten) {
            removeListener();
        }
    }
}

/**
 * @param {Window} [targetWindow]
 */
export function installDesktopFullscreenShortcut(targetWindow = window) {
    if (targetWindow !== window && targetWindow.top !== window) {
        return;
    }

    const targetDocument = targetWindow.document;
    if (installedDocuments.has(targetDocument)) {
        return;
    }

    const appWindow = getCurrentAppWindow();
    installedDocuments.add(targetDocument);
    targetDocument.addEventListener('keydown', (event) => {
        if (event.key !== 'F11'
            || event.altKey
            || event.ctrlKey
            || event.metaKey
            || event.shiftKey) {
            return;
        }

        event.preventDefault();
        if (event.repeat) {
            return;
        }

        fullscreenToggle = fullscreenToggle
            .then(async () => {
                const isFullscreen = await appWindow.isFullscreen();
                if (!isFullscreen) {
                    const [size, position] = await Promise.all([
                        appWindow.innerSize(),
                        appWindow.outerPosition(),
                    ]);
                    windowedGeometry = { size, position };
                }
                await appWindow.setFullscreen(!isFullscreen);
            })
            .catch((error) => {
                console.error('TauriTavern: Failed to toggle desktop fullscreen:', error);
            });
    }, true);
}
