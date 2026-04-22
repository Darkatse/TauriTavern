import { createAbortError } from '../kernel/abort-error.js';

function createRequestId() {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return crypto.randomUUID();
    }

    const timestamp = Date.now().toString(36);
    const random = Math.random().toString(36).slice(2, 10);
    return `${timestamp}-${random}`;
}

function normalizeSubpath(wildcard) {
    const value = String(wildcard || '').trim();
    return value.replace(/^\/+/, '');
}

function normalizeRouteResponse(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return { status: 500, kind: 'text', body: 'Invalid response' };
    }

    const kind = String(value.kind || '').trim().toLowerCase();
    const status = Number(value.status);
    const safeStatus = Number.isFinite(status) && status >= 100 && status <= 599 ? status : 500;

    if (kind === 'json') {
        return { status: safeStatus, kind: 'json', body: value.body ?? null };
    }

    if (kind === 'empty') {
        return { status: safeStatus, kind: 'empty', body: null };
    }

    return { status: safeStatus, kind: 'text', body: String(value.body ?? '') };
}

export function registerSdRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/sd/*', async ({ body, wildcard, init }) => {
        const path = normalizeSubpath(wildcard);
        const requestId = createRequestId();

        const signal = init?.signal;
        if (signal?.aborted) {
            throw createAbortError();
        }

        let abortRequested = false;
        let abortHandler = null;

        if (signal) {
            abortHandler = () => {
                abortRequested = true;
                void context.safeInvoke('cancel_sd_request', { requestId })
                    .catch((error) => {
                        console.debug('Failed to cancel SD request:', error);
                    });
            };
            signal.addEventListener('abort', abortHandler, { once: true });
        }

        try {
            const response = await context.safeInvoke('sd_handle', {
                requestId,
                path,
                body,
            });

            if (abortRequested) {
                throw createAbortError();
            }

            const normalized = normalizeRouteResponse(response);

            if (normalized.kind === 'json') {
                return jsonResponse(normalized.body, normalized.status);
            }

            if (normalized.kind === 'empty') {
                return new Response(null, { status: normalized.status });
            }

            return textResponse(normalized.body, normalized.status);
        } finally {
            if (signal && abortHandler) {
                signal.removeEventListener('abort', abortHandler);
            }
        }
    });
}
