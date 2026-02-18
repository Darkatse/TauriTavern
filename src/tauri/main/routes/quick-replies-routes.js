function parseQuickReplyPayload(body) {
    if (!body || typeof body !== 'object' || Array.isArray(body)) {
        return null;
    }

    return body;
}

function extractQuickReplyName(payload) {
    if (!payload) {
        return '';
    }

    return typeof payload.name === 'string' ? payload.name.trim() : '';
}

export function registerQuickReplyRoutes(router, context, { jsonResponse }) {
    const saveHandler = async ({ body }) => {
        const payload = parseQuickReplyPayload(body);
        const name = extractQuickReplyName(payload);
        if (!name) {
            return jsonResponse({ error: 'Quick Reply set name is required' }, 400);
        }

        await context.safeInvoke('save_quick_reply_set', { payload });
        return jsonResponse({ ok: true });
    };

    const deleteHandler = async ({ body }) => {
        const payload = parseQuickReplyPayload(body);
        const name = extractQuickReplyName(payload);
        if (!name) {
            return jsonResponse({ error: 'Quick Reply set name is required' }, 400);
        }

        await context.safeInvoke('delete_quick_reply_set', { payload });
        return jsonResponse({ ok: true });
    };

    router.post('/api/quick-replies/save', saveHandler);
    router.post('/api/quick-replies/delete', deleteHandler);

    // Legacy paths kept for compatibility with historical frontend calls.
    router.post('/savequickreply', saveHandler);
    router.post('/deletequickreply', deleteHandler);
}
