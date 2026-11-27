function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

export function registerProviderRoutes(router, context, { jsonResponse }) {
    router.post('/api/openrouter/models/providers', async ({ body }) => {
        const payload = asObject(body);
        const providers = await context.safeInvoke('get_openrouter_model_providers', {
            dto: { model: String(payload.model || '') },
        });
        return jsonResponse(providers);
    });

    router.post('/api/openrouter/credits', async () => {
        const credits = await context.safeInvoke('get_openrouter_credits');
        return jsonResponse(credits);
    });

    router.post('/api/nanogpt/models/providers', async ({ body }) => {
        const payload = asObject(body);
        const providers = await context.safeInvoke('get_nanogpt_model_providers', {
            dto: { model: String(payload.model || '') },
        });
        return jsonResponse(providers);
    });

    router.post('/api/nanogpt/credits', async () => {
        const credits = await context.safeInvoke('get_nanogpt_credits');
        return jsonResponse(credits);
    });
}
