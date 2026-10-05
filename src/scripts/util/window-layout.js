// Window geometry excludes the keyboard. Content viewport geometry stays in browser APIs.
import { hostPlatform, isMobileHost } from './host-identity.js';

let snapshot = null;
const subscribers = new Set();

export function getWindowSnapshot() { return snapshot; }

export function isWindowPortrait() {
    return snapshot ? snapshot.height >= snapshot.width : window.matchMedia('(orientation: portrait)').matches;
}

export function subscribeWindowOrientation(handler) {
    // OHOS keeps its browser orientation policy until its native window adapter lands.
    if (hostPlatform() === 'ohos') {
        const portrait = window.matchMedia('(orientation: portrait)');
        const changed = () => handler();
        portrait.addEventListener('change', changed);
        return () => portrait.removeEventListener('change', changed);
    }
    return subscribeWindowSnapshot(() => handler());
}

export function subscribeWindowSnapshot(handler) {
    subscribers.add(handler);
    if (snapshot) handler(snapshot);
    return () => subscribers.delete(handler);
}

export async function installWindowLayout(context) {
    if (!isMobileHost() || hostPlatform() === 'ohos') return;
    const receive = (next) => {
        if (!next || (snapshot && next.revision <= snapshot.revision)) return;
        snapshot = Object.freeze({ ...next, insets: Object.freeze(next.insets) });
        const background = document.getElementById('bg1');
        background.classList.add('tt-window-background');
        for (const [name, value] of Object.entries({
            x: -next.insets.left, y: -next.insets.top, width: next.width, height: next.height,
        })) {
            background.style.setProperty(`--tt-window-${name}`, `${value / next.scale}px`);
        }
        for (const handler of subscribers) handler(snapshot);
    };
    window.addEventListener('tt-window-changed', (event) => receive(event.detail));
    receive(await context.safeInvoke('get_window_snapshot'));
}
