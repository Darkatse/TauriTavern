const THIRD_PARTY_EXTENSION_PREFIX = '/scripts/extensions/third-party/';
const THUMBNAIL_ROUTE = '/thumbnail';
const CHARACTERS_ROUTE_PREFIX = '/characters/';
const BACKGROUNDS_ROUTE_PREFIX = '/backgrounds/';
const ASSETS_ROUTE_PREFIX = '/assets/';
const USER_IMAGES_ROUTE_PREFIX = '/user/images/';
const USER_FILES_ROUTE_PREFIX = '/user/files/';
const USER_AVATARS_ROUTE_PREFIX = '/User Avatars/';
const USER_AVATARS_ROUTE_PREFIX_ENCODED = '/User%20Avatars/';

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
    if (!shouldProxyRequestPath(requestUrl.pathname)) {
        return;
    }

    event.respondWith(proxyWebAssetRequest(event.request, requestUrl));
});

async function proxyWebAssetRequest(request, requestUrl) {
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
