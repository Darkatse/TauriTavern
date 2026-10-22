import { getWindowSnapshot, subscribeWindowSnapshot } from './util/window-layout.js';

let invoke = null;
let scheduled = false;
let lastWallpaper = null;
let lastColor = null;

/** First-party background writers use this entry, including chat locks and generated images. */
export function applyWindowBackdrop({ image, fitting } = {}) {
    const background = document.getElementById('bg1');
    if (image !== undefined) background.style.backgroundImage = image;
    if (fitting !== undefined) {
        for (const option of ['cover', 'contain', 'stretch', 'center']) {
            background.classList.toggle(option, option === fitting);
        }
    }
    if (!invoke || !getWindowSnapshot() || scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
        scheduled = false;
        publishBackdrop().catch(error => console.warn('[TauriTavern] Window backdrop:', error));
    });
}

export function installWindowBackdrop(context) {
    invoke = context.safeInvoke;
    subscribeWindowSnapshot(() => applyWindowBackdrop());
}

async function publishBackdrop() {
    const windowSnapshot = getWindowSnapshot();
    const style = getComputedStyle(document.getElementById('bg1'));
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const paint = canvas.getContext('2d');
    paint.fillStyle = 'white';
    paint.fillRect(0, 0, 1, 1);
    paint.fillStyle = getComputedStyle(document.body).backgroundColor;
    paint.fillRect(0, 0, 1, 1);
    const color = Array.from(paint.getImageData(0, 0, 1, 1).data).slice(0, 3);
    const visible = style.display !== 'none' && style.visibility === 'visible';
    let resourcePath = null;
    if (visible && style.backgroundImage !== 'none') {
        const match = /^url\(["']?(.*?)["']?\)$/.exec(style.backgroundImage);
        const url = match && new URL(match[1], location.href);
        if (url?.origin === location.origin) resourcePath = url.pathname;
    }
    // Keep the source URL in the key: a cache-busting query can replace bytes at the same path.
    const wallpaperKey = JSON.stringify([
        windowSnapshot.revision, visible, style.backgroundImage, style.backgroundSize, style.backgroundPosition,
    ]);
    const colorKey = color.join(',');
    const replaceWallpaper = wallpaperKey !== lastWallpaper?.key;
    if (!replaceWallpaper && colorKey === lastColor?.key) return;

    const request = {
        windowRevision: windowSnapshot.revision,
        color,
        wallpaper: replaceWallpaper
            ? { resourcePath, size: style.backgroundSize, position: style.backgroundPosition }
            : undefined,
    };
    if (replaceWallpaper) {
        if (visible && style.backgroundImage !== 'none' && !resourcePath) {
            console.warn('[TauriTavern] Native backdrop uses theme color for unsupported background:', style.backgroundImage);
        }
        lastWallpaper = { key: wallpaperKey, request };
    }
    lastColor = { key: colorKey, request };
    try {
        // Accept new state immediately. Only the Rust decoder is serialized.
        await invoke('set_window_backdrop', { request });
    } catch (error) {
        // A failed publication may retry, without invalidating a newer request's deduplication.
        if (lastWallpaper?.request === request) lastWallpaper = null;
        if (lastColor?.request === request) lastColor = null;
        throw error;
    }
}
