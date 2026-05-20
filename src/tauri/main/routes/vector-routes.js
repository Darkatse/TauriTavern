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

export function registerVectorRoutes(router, _context, { jsonResponse }) {
    router.post('/api/vector/*', async ({ wildcard }) => {
        const endpoint = normalizeEndpoint(wildcard);

        if (!SUPPORTED_VECTOR_ENDPOINTS.has(endpoint)) {
            return jsonResponse({ error: `Unsupported vector endpoint: ${endpoint}` }, 404);
        }

        return jsonResponse({
            error: true,
            cause: 'vector_endpoint_unavailable',
            message: 'Vector Storage backend is not implemented in the native TauriTavern backend yet.',
        }, 501);
    });
}
