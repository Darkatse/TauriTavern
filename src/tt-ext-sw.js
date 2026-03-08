const THIRD_PARTY_EXTENSION_PREFIX = '/scripts/extensions/third-party/';

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

    return new URL('http://tt-ext.localhost/');
}

const ttExtBaseUrl = resolveTtExtBaseUrl();

self.addEventListener('install', (event) => {
    event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', (event) => {
    event.waitUntil(self.clients.claim());
});

self.addEventListener('fetch', (event) => {
    const requestUrl = new URL(event.request.url);
    if (!requestUrl.pathname.startsWith(THIRD_PARTY_EXTENSION_PREFIX)) {
        return;
    }

    event.respondWith(proxyThirdPartyExtensionRequest(event.request, requestUrl));
});

async function proxyThirdPartyExtensionRequest(request, requestUrl) {
    const targetUrl = new URL(`${requestUrl.pathname}${requestUrl.search}`, ttExtBaseUrl);

    const init = { method: request.method, credentials: 'omit' };

    if (request.method !== 'GET' && request.method !== 'HEAD') {
        init.body = await request.clone().arrayBuffer();
    }

    const upstream = await fetch(targetUrl.href, init);

    return new Response(upstream.body, {
        status: upstream.status,
        statusText: upstream.statusText,
        headers: upstream.headers,
    });
}
