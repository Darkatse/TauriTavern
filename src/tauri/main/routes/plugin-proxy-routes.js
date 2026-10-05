import { decodeBase64ToBytes } from '../binary-utils.js';
import { jsonResponse, safeResponseStatusText } from '../http-utils.js';
import { extractErrorText } from '../kernel/host-error-response.js';
import { readBridgeConfig, readBridgeCredentials } from '../services/plugin-bridge/bridge-config.js';

/**
 * Reverse proxy for SillyTavern server plugins hosted by an external
 * SillyTavern instance (one with `enableServerPlugins: true` in config.yaml).
 *
 * In Tauri mode unmatched `/api/plugins/*` requests never leave the WebView,
 * so server plugins are unreachable natively. This route forwards them through
 * the `plugin_proxy_request` command, which runs on the native HTTP stack and
 * is therefore not subject to WebView CORS restrictions.
 *
 * Configuration lives in localStorage and is read through
 * services/plugin-bridge/bridge-config.js (per-device values: the desktop
 * talks to 127.0.0.1 while mobile needs the desktop's LAN address, so the
 * synced tauritavern-settings.json must not own them).
 *
 * Rust invoke contract (`plugin_proxy_commands.rs`):
 *   locale: string,                     // navigator.language, for the endpoint-access dialog
 *   args.request = {
 *     baseUrl: string,                    // no trailing slash
 *     method: string,                     // uppercase
 *     pathAndQuery: string,               // must start with /api/plugins/
 *     headers: Array<[string, string]>,   // lowercased, hop-by-hop headers stripped
 *     body: string | null,                // raw UTF-8 text
 *     timeoutMs: number,
 *     maxResponseBytes: number,
 *   }
 *   returns { status, statusText, headers: Array<[string, string]>, bodyBase64 }
 *
 * Boundaries: request bodies are forwarded as UTF-8 text only (server plugin
 * APIs are JSON); FormData, URLSearchParams, and binary bodies are rejected
 * with 400 instead of being dropped. Browser-managed credentials (HTTP Basic
 * auth prompts) are not visible to fetch and are not forwarded.
 */

const PLUGIN_API_PREFIX = '/api/plugins/';
const DEFAULT_TIMEOUT_MS = 120_000;
const MAX_RESPONSE_BYTES = 64 * 1024 * 1024;

const STRIPPED_REQUEST_HEADERS = new Set([
    'host',
    'content-length',
    'connection',
    'keep-alive',
    'transfer-encoding',
    'upgrade',
    'expect',
    'accept-encoding',
    'content-encoding',
    'cookie',
]);

const STRIPPED_RESPONSE_HEADERS = new Set([
    'content-length',
    'connection',
    'keep-alive',
    'transfer-encoding',
    'content-encoding',
    'set-cookie',
]);

function collectRequestHeaders(input, init) {
    const source = init?.headers ?? (input && typeof input === 'object' ? input.headers : null);
    if (!source) {
        return [];
    }

    const headers = [];
    let entries;
    try {
        entries = new Headers(source).entries();
    } catch {
        return headers;
    }

    for (const [name, value] of entries) {
        if (STRIPPED_REQUEST_HEADERS.has(name.toLowerCase())) {
            continue;
        }
        headers.push([name.toLowerCase(), String(value)]);
    }
    return headers;
}

function encodeUtf8Base64(value) {
    const bytes = new TextEncoder().encode(value);
    let binary = '';
    for (const byte of bytes) {
        binary += String.fromCharCode(byte);
    }
    return btoa(binary);
}

function buildRequestHeaders(input, init) {
    const headers = collectRequestHeaders(input, init);
    const hasAuthorization = headers.some(([name]) => name === 'authorization');
    if (!hasAuthorization) {
        const credentials = readBridgeCredentials();
        if (credentials) {
            // The browser's native Basic-auth cache is invisible to the proxy,
            // so configured credentials stand in for it (a host with
            // basicAuthMode: true rejects unauthenticated plugin requests).
            headers.push(['authorization', `Basic ${encodeUtf8Base64(credentials)}`]);
        }
    }
    return headers;
}

function buildProxyResponse(payload) {
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        throw new Error('Invalid plugin proxy response payload');
    }

    const status = Number(payload.status);
    if (!Number.isFinite(status) || status < 100 || status > 599) {
        // A malformed payload means the IPC contract is broken: route it into
        // the caller's catch path so it gets logged instead of being silently
        // normalized into a 502.
        throw new Error(`Invalid plugin proxy response status: ${payload.status}`);
    }
    const headers = new Headers();
    for (const entry of Array.isArray(payload.headers) ? payload.headers : []) {
        if (!Array.isArray(entry) || entry.length < 2) {
            continue;
        }
        const [name, value] = entry;
        if (typeof name !== 'string' || STRIPPED_RESPONSE_HEADERS.has(name.toLowerCase())) {
            continue;
        }
        try {
            headers.append(name, String(value));
        } catch {
            // Ignore malformed header pairs from the upstream host.
        }
    }

    return new Response(decodeBase64ToBytes(String(payload.bodyBase64 ?? '')), {
        status,
        headers,
        statusText: safeResponseStatusText(payload.statusText),
    });
}

async function handlePluginProxyRequest(context, request) {
    const config = readBridgeConfig();
    if (!config.enabled) {
        // Fall through so the dispatcher answers with the standard
        // unsupported-endpoint 404, matching pre-bridge behavior.
        return undefined;
    }

    const suffix = String(request.wildcard || '').replace(/^\/+/, '');
    if (!suffix) {
        return jsonResponse({ error: 'Server plugin id is required' }, 400);
    }

    const pathAndQuery = `${PLUGIN_API_PREFIX}${suffix}${request.url?.search ?? ''}`;
    const rawBody = request.body;
    let bodyText = null;
    if (typeof rawBody === 'string') {
        bodyText = rawBody.length > 0 ? rawBody : null;
    } else if (rawBody !== null && rawBody !== undefined) {
        // Fail fast: FormData, URLSearchParams, and object bodies cannot cross
        // the text-only bridge contract. Dropping them silently would send an
        // empty-body request upstream and report success to the caller.
        return jsonResponse(
            {
                error: 'Plugin bridge only forwards UTF-8 text request bodies',
                received: typeof rawBody,
            },
            400,
        );
    }

    try {
        const payload = await context.safeInvoke('plugin_proxy_request', {
            locale: typeof navigator !== 'undefined' && navigator.language ? navigator.language : 'en',
            request: {
                baseUrl: config.baseUrl,
                method: String(request.method || 'GET').toUpperCase(),
                pathAndQuery,
                headers: buildRequestHeaders(request.input, request.init),
                body: bodyText,
                timeoutMs: DEFAULT_TIMEOUT_MS,
                maxResponseBytes: MAX_RESPONSE_BYTES,
            },
        });
        return buildProxyResponse(payload);
    } catch (error) {
        // Connection failures are mapped to a 502 payload by the Rust side;
        // reaching this path usually means the command is unavailable (older
        // backend) or the IPC layer failed. Either way a non-2xx status reads
        // as "backend absent" to probe-driven extensions.
        console.warn('TauriTavern plugin proxy failed', {
            path: pathAndQuery,
            message: extractErrorText(error),
        });
        return jsonResponse(
            { error: 'Plugin bridge unreachable', message: extractErrorText(error) },
            502,
        );
    }
}

export function registerPluginProxyRoutes(router, context) {
    router.all(
        '/api/plugins/*',
        (request) => handlePluginProxyRequest(context, request),
        { body: 'text' },
    );
}
