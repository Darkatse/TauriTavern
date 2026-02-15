const NATIVE_SHARE_BRIDGE_KEY = '__TAURITAVERN_NATIVE_SHARE__';

function normalizeSharePayload(rawPayload) {
    if (!rawPayload || typeof rawPayload !== 'object') {
        return null;
    }

    const kind = String(rawPayload.kind || rawPayload.type || '').trim().toLowerCase();

    if (kind === 'url') {
        const value = String(rawPayload.url || '').trim();
        if (!value) {
            return null;
        }

        try {
            const parsed = new URL(value);
            const protocol = parsed.protocol.toLowerCase();
            if (protocol !== 'http:' && protocol !== 'https:') {
                return null;
            }

            return {
                kind: 'url',
                url: parsed.toString(),
            };
        } catch {
            return null;
        }
    }

    if (kind === 'png') {
        const path = String(rawPayload.path || '').trim();
        if (!path) {
            return null;
        }

        const fileName = String(rawPayload.fileName || rawPayload.name || 'shared-character.png').trim() || 'shared-character.png';
        const mimeType = String(rawPayload.mimeType || 'image/png').trim() || 'image/png';

        return {
            kind: 'png',
            path,
            fileName,
            mimeType,
        };
    }

    return null;
}

export function installNativeShareBridge() {
    if (typeof window === 'undefined') {
        return null;
    }

    const existing = window[NATIVE_SHARE_BRIDGE_KEY];
    if (existing && typeof existing.push === 'function' && typeof existing.subscribe === 'function') {
        return existing;
    }

    const pendingPayloads = [];
    const subscribers = new Set();

    function deliver(payload) {
        for (const subscriber of subscribers) {
            try {
                subscriber(payload);
            } catch (error) {
                console.error('Failed to dispatch native share payload:', error);
            }
        }
    }

    const bridge = {
        push(rawPayload) {
            const payload = normalizeSharePayload(rawPayload);
            if (!payload) {
                return false;
            }

            if (subscribers.size === 0) {
                pendingPayloads.push(payload);
                return true;
            }

            deliver(payload);
            return true;
        },
        subscribe(handler) {
            if (typeof handler !== 'function') {
                return () => { /* noop */ };
            }

            subscribers.add(handler);

            if (pendingPayloads.length > 0) {
                const backlog = pendingPayloads.splice(0, pendingPayloads.length);
                for (const payload of backlog) {
                    try {
                        handler(payload);
                    } catch (error) {
                        console.error('Failed to handle queued native share payload:', error);
                    }
                }
            }

            return () => subscribers.delete(handler);
        },
    };

    window[NATIVE_SHARE_BRIDGE_KEY] = bridge;
    return bridge;
}
