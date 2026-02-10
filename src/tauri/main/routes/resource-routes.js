export function registerResourceRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/avatars/get', async () => {
        const avatars = await context.safeInvoke('get_avatars');
        return jsonResponse(Array.isArray(avatars) ? avatars : []);
    });

    router.post('/api/avatars/delete', async ({ body }) => {
        await context.safeInvoke('delete_avatar', { avatar: body?.avatar || '' });
        return jsonResponse({ result: 'ok' });
    });

    router.post('/api/avatars/upload', async ({ body, url }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const result = await context.uploadAvatarFromForm(body, url);
        return jsonResponse(result || {});
    });

    router.post('/api/backgrounds/all', async () => {
        const images = await context.safeInvoke('get_all_backgrounds');
        return jsonResponse({
            images: Array.isArray(images) ? images : [],
            config: { width: 160, height: 90 },
        });
    });

    router.post('/api/backgrounds/delete', async ({ body }) => {
        await context.safeInvoke('delete_background', { dto: { bg: body?.bg || '' } });
        return jsonResponse({ ok: true });
    });

    router.post('/api/backgrounds/rename', async ({ body }) => {
        await context.safeInvoke('rename_background', {
            dto: {
                old_bg: body?.old_bg || '',
                new_bg: body?.new_bg || '',
            },
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/backgrounds/upload', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: 'No background file provided' }, 400);
        }

        const filename = file instanceof File ? file.name : 'background.png';
        const data = Array.from(new Uint8Array(await file.arrayBuffer()));
        const uploaded = await context.safeInvoke('upload_background', { filename, data });

        return textResponse(String(uploaded || filename));
    });

    router.post('/api/themes/save', async ({ body }) => {
        await context.safeInvoke('save_theme', { dto: body || {} });
        return jsonResponse({ ok: true });
    });

    router.post('/api/themes/delete', async ({ body }) => {
        await context.safeInvoke('delete_theme', { dto: { name: body?.name || '' } });
        return jsonResponse({ ok: true });
    });

    router.post('/api/groups/all', async () => {
        const groups = await context.safeInvoke('get_all_groups');
        return jsonResponse(Array.isArray(groups) ? groups : []);
    });

    router.post('/api/groups/get', async ({ body }) => {
        const group = await context.safeInvoke('get_group', { id: body?.id || '' });
        return jsonResponse(group || null);
    });

    router.post('/api/groups/create', async ({ body }) => {
        const created = await context.safeInvoke('create_group', { dto: body || {} });
        return jsonResponse(created || {});
    });

    router.post('/api/groups/edit', async ({ body }) => {
        const updated = await context.safeInvoke('update_group', { dto: body || {} });
        return jsonResponse(updated || {});
    });

    router.post('/api/groups/delete', async ({ body }) => {
        await context.safeInvoke('delete_group', { dto: { id: body?.id || '' } });
        return jsonResponse({ ok: true });
    });
}
