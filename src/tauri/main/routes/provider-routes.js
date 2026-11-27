function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function providerIds(providers) {
    return providers.map(provider => provider.id);
}

export function registerProviderRoutes(router, context, { jsonResponse }) {
    router.post('/api/openrouter/models/providers', async ({ body }) => {
        const payload = asObject(body);
        const providers = await context.safeInvoke('get_openrouter_model_providers', {
            dto: { model: String(payload.model || '') },
        });
        return jsonResponse(providerIds(providers));
    });

    router.post('/api/openrouter/credits', async () => {
        const credits = await context.safeInvoke('get_openrouter_credits');
        return jsonResponse(credits);
    });

    router.post('/api/nanogpt/models/providers', async ({ body }) => {
        const payload = asObject(body);
        const selection = await context.safeInvoke('get_nanogpt_model_providers', {
            dto: { model: String(payload.model || '') },
        });
        return jsonResponse({
            supportsProviderSelection: selection.supportsProviderSelection,
            providers: providerIds(selection.providers),
        });
    });

    router.post('/api/nanogpt/credits', async () => {
        const credits = await context.safeInvoke('get_nanogpt_credits');
        return jsonResponse(credits);
    });
}
