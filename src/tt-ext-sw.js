const THIRD_PARTY_EXTENSION_PREFIX = '/scripts/extensions/third-party/';
const THUMBNAIL_ROUTE = '/thumbnail';
const CHARACTERS_ROUTE_PREFIX = '/characters/';
const BACKGROUNDS_ROUTE_PREFIX = '/backgrounds/';
const ASSETS_ROUTE_PREFIX = '/assets/';
const USER_IMAGES_ROUTE_PREFIX = '/user/images/';
const USER_FILES_ROUTE_PREFIX = '/user/files/';
const USER_AVATARS_ROUTE_PREFIX = '/User Avatars/';
const USER_AVATARS_ROUTE_PREFIX_ENCODED = '/User%20Avatars/';

const PROXY_REQUEST_MESSAGE_TYPE = 'tt-ext-proxy-request';
const PROXY_READY_MESSAGE_TYPE = 'tt-ext-proxy-ready';
const CLIENT_BRIDGE_TIMEOUT_MS = 8000;

function shouldProxyRequestPath(pathname) {
    return pathname === THUMBNAIL_ROUTE
        || pathname.startsWith(THIRD_PARTY_EXTENSION_PREFIX)
        || pathname.startsWith(CHARACTERS_ROUTE_PREFIX)
        || pathname.startsWith(BACKGROUNDS_ROUTE_PREFIX)
        || pathname.startsWith(ASSETS_ROUTE_PREFIX)
        || pathname.startsWith(USER_IMAGES_ROUTE_PREFIX)
        || pathname.startsWith(USER_FILES_ROUTE_PREFIX)
        || pathname.startsWith(USER_AVATARS_ROUTE_PREFIX_ENCODED)
        || pathname.startsWith(USER_AVATARS_ROUTE_PREFIX);
}

function resolveTtExtBaseUrl() {
    try {
        const swUrl = new URL(self.location.href);
        const base = swUrl.searchParams.get('base');
        if (base) {
            return new URL(String(base).trim());
        }
    } catch {
        // Ignore invalid base URL.
    }

    return new URL('tt-ext://localhost/');
}

const ttExtBaseUrl = resolveTtExtBaseUrl();
let proxyFallbackLogged = false;
let proxyViaClient = false;
let proxyBridgeClientId = null;

self.addEventListener('message', (event) => {
    const data = event?.data;
    if (!data || data.type !== PROXY_READY_MESSAGE_TYPE) {
        return;
    }

    const source = event?.source;
    const clientId = source && typeof source.id === 'string' ? source.id : null;
    if (clientId) {
        proxyBridgeClientId = clientId;
    }
});

self.addEventListener('install', (event) => {
    event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', (event) => {
    event.waitUntil(self.clients.claim());
});

self.addEventListener('fetch', (event) => {
    const requestUrl = new URL(event.request.url);
    if (!shouldProxyRequestPath(requestUrl.pathname)) {
        return;
    }

    event.respondWith(proxyWebAssetRequest(event, requestUrl));
});

async function proxyWebAssetRequest(event, requestUrl) {
    const request = event.request;
    const targetUrl = new URL(`${requestUrl.pathname}${requestUrl.search}`, ttExtBaseUrl);

    const init = { method: request.method, credentials: 'omit' };

    if (request.method !== 'GET' && request.method !== 'HEAD') {
        init.body = await request.clone().arrayBuffer();
    }

    if (proxyViaClient) {
        return proxyViaClientBridge(event, requestUrl, init, new TypeError('Service Worker proxy fetch is disabled'));
    }

    try {
        const upstream = await fetch(targetUrl.href, init);

        return new Response(upstream.body, {
            status: upstream.status,
            statusText: upstream.statusText,
            headers: new Headers(upstream.headers),
        });
    } catch (error) {
        proxyViaClient = true;
        // WebKit may fail to fetch from custom schemes inside a Service Worker.
        // Fallback to a window-context fetch via postMessage.
        if (!proxyFallbackLogged) {
            proxyFallbackLogged = true;
            console.warn('TauriTavern SW: proxy fetch failed, falling back to client bridge:', error);
        }

        return proxyViaClientBridge(event, requestUrl, init, error);
    }
}

async function proxyViaClientBridge(event, requestUrl, init, originalError) {
    const candidates = await resolveProxyClients(event);
    if (!candidates || candidates.length === 0) {
        throw originalError;
    }

    let lastError = originalError;
    for (const client of candidates) {
        try {
            const payload = await sendProxyRequestToClient(client, requestUrl, init, originalError);
            const headers = new Headers(payload.headers || []);
            return new Response(payload.body || null, {
                status: payload.status || 200,
                statusText: payload.statusText || '',
                headers,
            });
        } catch (error) {
            lastError = error || lastError;
        }
    }

    throw lastError || originalError;
}

function sendProxyRequestToClient(client, requestUrl, init, originalError) {
    if (!client || typeof client.postMessage !== 'function') {
        return Promise.reject(originalError);
    }

    return new Promise((resolve, reject) => {
        const channel = new MessageChannel();
        let timeoutId = null;
        let settled = false;

        const cleanup = () => {
            if (timeoutId !== null) {
                clearTimeout(timeoutId);
                timeoutId = null;
            }
            try {
                channel.port1.onmessage = null;
                channel.port1.onmessageerror = null;
                channel.port1.close();
            } catch {
                // Ignore.
            }
        };

        const succeed = (payload) => {
            if (settled) {
                return;
            }
            settled = true;
            cleanup();
            resolve(payload);
        };

        const fail = (error) => {
            if (settled) {
                return;
            }
            settled = true;
            cleanup();
            reject(error);
        };

        channel.port1.onmessage = (messageEvent) => {
            const payload = messageEvent?.data;
            if (!payload || payload.ok !== true) {
                fail(payload?.error ? new Error(payload.error) : originalError);
                return;
            }
            succeed(payload);
        };
        channel.port1.onmessageerror = () => {
            fail(originalError);
        };

        timeoutId = setTimeout(() => {
            fail(new Error(`Service Worker client bridge timed out (${CLIENT_BRIDGE_TIMEOUT_MS}ms)`));
        }, CLIENT_BRIDGE_TIMEOUT_MS);

        try {
            client.postMessage({
                type: PROXY_REQUEST_MESSAGE_TYPE,
                pathname: requestUrl.pathname,
                search: requestUrl.search,
                method: init.method,
            }, [channel.port2]);
        } catch (error) {
            fail(error);
        }
    });
}

function isThirdPartyExtensionClient(client) {
    try {
        const url = new URL(client.url);
        return url.pathname.startsWith(THIRD_PARTY_EXTENSION_PREFIX);
    } catch {
        return false;
    }
}

function scoreProxyClient(client) {
    let score = 0;
    if (!client) {
        return score;
    }
    if (proxyBridgeClientId && client.id === proxyBridgeClientId) {
        score += 1000;
    }
    if (client.focused) {
        score += 50;
    }
    if (client.visibilityState === 'visible') {
        score += 20;
    }
    if (client.frameType === 'top-level') {
        score += 10;
    }
    if (isThirdPartyExtensionClient(client)) {
        score -= 100;
    }
    return score;
}

async function resolveProxyClients(event) {
    const seen = new Set();
    const candidates = [];

    const push = (client) => {
        if (!client || typeof client.id !== 'string') {
            return;
        }
        if (seen.has(client.id)) {
            return;
        }
        seen.add(client.id);
        candidates.push(client);
    };

    try {
        if (proxyBridgeClientId) {
            const bridgeClient = await self.clients.get(proxyBridgeClientId);
            push(bridgeClient);
        }
    } catch {
        // Ignore.
    }

    try {
        const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
        if (!clients || clients.length === 0) {
            return candidates;
        }

        const sorted = [...clients].sort((a, b) => scoreProxyClient(b) - scoreProxyClient(a));
        for (const client of sorted) {
            push(client);
        }
    } catch {
        // Ignore.
    }

    try {
        if (event.clientId) {
            const requestingClient = await self.clients.get(event.clientId);
            push(requestingClient);
        }
    } catch {
        // Ignore.
    }

    return candidates;
}
