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

    router.post('/api/openai/siliconflow/models/embedding', async ({ body }) => {
        const payload = asObject(body);
        const models = await context.safeInvoke('get_siliconflow_embedding_models', {
            dto: { siliconflow_endpoint: String(payload.siliconflow_endpoint || '') },
        });
        return jsonResponse(models);
    });

    router.post('/api/openai/workers-ai/models/embedding', async ({ body }) => {
        const payload = asObject(body);
        const models = await context.safeInvoke('get_workers_ai_embedding_models', {
            dto: { workers_ai_account_id: String(payload.workers_ai_account_id || '') },
        });
        return jsonResponse(models);
    });

    router.post('/api/backends/chat-completions/multimodal-models/workers_ai', async ({ body }) => {
        const payload = asObject(body);
        const models = await context.safeInvoke('get_workers_ai_multimodal_models', {
            dto: { workers_ai_account_id: String(payload.workers_ai_account_id || '') },
        });
        return jsonResponse(models);
    });
}
