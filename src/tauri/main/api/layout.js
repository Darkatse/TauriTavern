// @ts-check

/**
 * Layout describes the already usable browser viewport. Native system bars and
 * docked keyboards have been consumed before this document is laid out.
 */
function readSnapshot() {
    const visual = window.visualViewport;
    const left = visual?.offsetLeft ?? 0;
    const top = visual?.offsetTop ?? 0;
    const width = visual?.width ?? window.innerWidth;
    const height = visual?.height ?? window.innerHeight;
    const viewport = { left, top, width, height, right: left + width, bottom: top + height };
    return {
        version: 2,
        timestampMs: Date.now(),
        viewport,
        safeInsets: { top: 0, right: 0, bottom: 0, left: 0 },
        safeFrame: { ...viewport },
        ime: { bottom: 0, viewportBottomInset: 0, keyboardOffset: 0 },
    };
}

export function installLayoutApi() {
    const hostAbi = window.__TAURITAVERN__;
    if (!hostAbi) throw new Error('Host ABI __TAURITAVERN__ is missing');
    hostAbi.api ??= {};
    const subscribers = new Set();
    let scheduled = false;
    const deliver = (handler, snapshot) => {
        try { handler(snapshot); }
        catch (error) { console.error('[TauriTavern] layout subscriber failed.', error); }
    };
    const schedule = () => {
        if (scheduled) return;
        scheduled = true;
        requestAnimationFrame(() => {
            scheduled = false;
            const snapshot = readSnapshot();
            for (const handler of subscribers) deliver(handler, snapshot);
        });
    };
    hostAbi.api.layout = {
        snapshot: readSnapshot,
        async subscribe(handler) {
            if (typeof handler !== 'function') throw new TypeError('handler must be a function');
            if (subscribers.size === 0) {
                window.addEventListener('resize', schedule);
                window.visualViewport?.addEventListener('resize', schedule);
                window.visualViewport?.addEventListener('scroll', schedule);
            }
            subscribers.add(handler);
            deliver(handler, readSnapshot());
            return async () => {
                subscribers.delete(handler);
                if (subscribers.size === 0) {
                    window.removeEventListener('resize', schedule);
                    window.visualViewport?.removeEventListener('resize', schedule);
                    window.visualViewport?.removeEventListener('scroll', schedule);
                }
            };
        },
    };
}
