import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

const SUPPORTED_VECTOR_ENDPOINTS = new Set([
    'list',
    'insert',
    'delete',
    'query',
    'query-multi',
    'purge',
    'purge-all',
]);

function normalizeEndpoint(wildcard) {
    return String(wildcard || '').replace(/^\/+/, '');
}

function vectorErrorCause(status) {
    if (status === 401) return 'embedding_auth_failed';
    if (status === 409) return 'vector_index_conflict';
    if (status === 429) return 'embedding_rate_limited';
    if (status >= 500) return 'embedding_unavailable';
    return undefined;
}

async function invokeVector(context, jsonResponse, path, request) {
    try {
        const response = await context.safeInvoke('vector_handle', { path, request });
        const status = Number(response?.status);
        const safeStatus = Number.isInteger(status) && status >= 100 && status <= 599 ? status : 500;

        if (response?.kind === 'empty') {
            return new Response(null, { status: safeStatus });
        }
        if (response?.kind === 'json') {
            return jsonResponse(response.body ?? null, safeStatus);
        }
        return jsonResponse({ error: true, cause: 'embedding_unavailable', message: 'Invalid native vector response' }, 500);
    } catch (error) {
        const resolved = resolveHostErrorResponse(extractErrorText(error));
        return jsonResponse({
            error: true,
            cause: vectorErrorCause(resolved.status),
            message: resolved.body,
        }, resolved.status);
    }
}

export function registerVectorRoutes(router, context, { jsonResponse }) {
    router.post('/api/vector/*', async ({ body, wildcard }) => {
        const endpoint = normalizeEndpoint(wildcard);

        if (!SUPPORTED_VECTOR_ENDPOINTS.has(endpoint)) {
            return jsonResponse({ error: `Unsupported vector endpoint: ${endpoint}` }, 404);
        }

        return invokeVector(
            context,
            jsonResponse,
            endpoint,
            body && typeof body === 'object' && !Array.isArray(body) ? body : {},
        );
    });

    router.post('/api/backends/kobold/embed', async ({ body }) => {
        return invokeVector(context, jsonResponse, 'koboldcpp-embed', {
            source: 'koboldcpp',
            apiUrl: body?.server,
            ...(body?.isQuery === true ? { isQuery: true } : {}),
            texts: body?.items,
        });
    });
}
