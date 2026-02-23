import { decodeBase64ToBytes } from '../binary-utils.js';

function decodeRoutePath(value) {
    try {
        return decodeURIComponent(String(value || ''));
    } catch {
        return String(value || '');
    }
}

function isNotFoundError(error) {
    const message = String(error?.message || error || '').toLowerCase();
    return message.includes('not found')
        || message.includes('no such file')
        || message.includes('enoent')
        || message.includes('os error 2');
}

function sanitizeFileName(value) {
    return String(value || '')
        .replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '_')
        .replace(/[. ]+$/g, '')
        .trim();
}

export function registerResourceRoutes(router, context, { jsonResponse, textResponse }) {
    router.post('/api/files/sanitize-filename', async ({ body }) => {
        const sanitized = sanitizeFileName(body?.fileName || '');

        if (!sanitized) {
            return jsonResponse({ error: 'Invalid filename' }, 400);
        }

        return jsonResponse({ fileName: sanitized });
    });

    router.post('/api/files/upload', async ({ body }) => {
        const name = String(body?.name || '').trim();
        const data = String(body?.data || '').trim();

        if (!name) {
            return jsonResponse({ error: 'No upload name specified' }, 400);
        }

        if (!data) {
            return jsonResponse({ error: 'No upload data specified' }, 400);
        }

        const uploaded = await context.safeInvoke('upload_user_file', {
            name,
            data_base64: data,
        });
        return jsonResponse(uploaded || {});
    });

    router.post('/api/files/delete', async ({ body }) => {
        const path = String(body?.path || '').trim();
        if (!path) {
            return textResponse('No path specified', 400);
        }

        try {
            await context.safeInvoke('delete_user_file', { path });
            return jsonResponse({ ok: true });
        } catch (error) {
            if (isNotFoundError(error)) {
                return textResponse('File not found', 404);
            }

            throw error;
        }
    });

    router.post('/api/files/verify', async ({ body }) => {
        if (!Array.isArray(body?.urls)) {
            return textResponse('No URLs specified', 400);
        }

        const urls = body.urls.map((url) => String(url || '').trim()).filter(Boolean);
        const verified = await context.safeInvoke('verify_user_files', { urls });
        return jsonResponse(verified && typeof verified === 'object' ? verified : {});
    });

    router.get('/user/files/*', async ({ wildcard }) => {
        const relativePath = decodeRoutePath(wildcard).replace(/^\/+/, '');
        if (!relativePath) {
            return textResponse('Not Found', 404);
        }

        try {
            const payload = await context.safeInvoke('read_user_file_asset', {
                relative_path: relativePath,
            });
            const bytes = decodeBase64ToBytes(payload?.content_base64 || '');
            return new Response(bytes, {
                status: 200,
                headers: {
                    'Content-Type': payload?.mime_type || 'application/octet-stream',
                    'Cache-Control': 'no-store',
                },
            });
        } catch (error) {
            if (isNotFoundError(error)) {
                return textResponse('Not Found', 404);
            }

            throw error;
        }
    });

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

        const rawFilename = file instanceof File ? file.name : 'background.png';
        const filename = sanitizeFileName(rawFilename);
        if (!filename) {
            return jsonResponse({ error: 'Invalid filename' }, 400);
        }

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
