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

function parseCommandErrorStatus(error) {
    const message = String(error?.message || error || '');
    if (message.startsWith('Bad request:')) {
        return 400;
    }
    if (message.startsWith('Not found:')) {
        return 404;
    }
    if (message.startsWith('Unauthorized:')) {
        return 401;
    }
    if (message.startsWith('Too many requests:')) {
        return 429;
    }
    return 500;
}

function stripCommandErrorPrefix(error) {
    const message = String(error?.message || error || '').trim();
    for (const prefix of ['Bad request:', 'Not found:', 'Unauthorized:', 'Too many requests:', 'Internal server error:']) {
        if (message.startsWith(prefix)) {
            return message.slice(prefix.length).trim();
        }
    }
    return message;
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

    router.post('/api/images/upload', async ({ body }) => {
        if (!body || typeof body !== 'object' || Array.isArray(body)) {
            return jsonResponse({ error: 'No data provided' }, 400);
        }

        const image = typeof body.image === 'string' ? body.image.trim() : '';
        if (!image) {
            return jsonResponse({ error: 'No image data provided' }, 400);
        }

        try {
            const uploaded = await context.safeInvoke('upload_user_image', {
                image_base64: image,
                format: body.format,
                filename: body.filename,
                ch_name: body.ch_name,
            });
            return jsonResponse(uploaded || {});
        } catch (error) {
            const status = parseCommandErrorStatus(error);
            const message = stripCommandErrorPrefix(error);
            if (status === 400) {
                return jsonResponse({ error: message || 'Failed to save the image' }, 400);
            }
            return jsonResponse({ error: 'Failed to save the image' }, 500);
        }
    });

    router.post('/api/images/list', async ({ body }) => {
        if (body?.folder === undefined || body?.folder === null || String(body.folder).trim() === '') {
            return jsonResponse({ error: 'No folder specified' }, 400);
        }

        try {
            const images = await context.safeInvoke('list_user_images', {
                folder: body.folder,
                media_type: body.type,
                sort_field: body.sortField,
                sort_order: body.sortOrder,
            });
            return jsonResponse(Array.isArray(images) ? images : []);
        } catch (error) {
            const status = parseCommandErrorStatus(error);
            const message = stripCommandErrorPrefix(error);
            if (status === 400) {
                return jsonResponse({ error: message || 'Unable to retrieve files' }, 400);
            }
            return jsonResponse({ error: 'Unable to retrieve files' }, 500);
        }
    });

    router.post('/api/images/list/*', async ({ body, wildcard }) => {
        if (wildcard && body?.folder) {
            return jsonResponse({ error: 'Folder specified in both URL and body' }, 400);
        }

        const folder = wildcard ? wildcard.replace(/^\/+/, '') : (body?.folder ?? '');
        if (!folder || String(folder).trim() === '') {
            return jsonResponse({ error: 'No folder specified' }, 400);
        }

        try {
            const images = await context.safeInvoke('list_user_images', {
                folder,
                media_type: body?.type,
                sort_field: body?.sortField,
                sort_order: body?.sortOrder,
            });
            return jsonResponse(Array.isArray(images) ? images : []);
        } catch (error) {
            const status = parseCommandErrorStatus(error);
            const message = stripCommandErrorPrefix(error);
            if (status === 400) {
                return jsonResponse({ error: message || 'Unable to retrieve files' }, 400);
            }
            return jsonResponse({ error: 'Unable to retrieve files' }, 500);
        }
    });

    router.post('/api/images/folders', async () => {
        try {
            const folders = await context.safeInvoke('list_user_image_folders');
            return jsonResponse(Array.isArray(folders) ? folders : []);
        } catch (error) {
            const status = parseCommandErrorStatus(error);
            const message = stripCommandErrorPrefix(error);
            if (status === 400) {
                return jsonResponse({ error: message || 'Unable to retrieve folders' }, 400);
            }
            return jsonResponse({ error: 'Unable to retrieve folders' }, 500);
        }
    });

    router.post('/api/images/delete', async ({ body }) => {
        const path = String(body?.path || '').trim();
        if (!path) {
            return textResponse('No path specified', 400);
        }

        try {
            await context.safeInvoke('delete_user_image', { path });
            return jsonResponse({ ok: true });
        } catch (error) {
            if (isNotFoundError(error)) {
                return textResponse('File not found', 404);
            }

            const status = parseCommandErrorStatus(error);
            const message = stripCommandErrorPrefix(error);
            if (status === 400) {
                return textResponse(message || 'Invalid path', 400);
            }
            return textResponse('Internal Server Error', 500);
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

    router.post('/api/image-metadata/all', async ({ body }) => {
        const prefix = typeof body?.prefix === 'string' ? body.prefix : '';
        const payload = await context.safeInvoke('get_all_background_metadata', { prefix });
        return jsonResponse(payload && typeof payload === 'object'
            ? payload
            : { version: 1, images: {} });
    });

    router.post('/api/backgrounds/delete', async ({ body }) => {
        await context.safeInvoke('delete_background', { dto: { bg: body?.bg || '' } });
        context.invalidateInvokeAll('read_thumbnail_asset');
        return jsonResponse({ ok: true });
    });

    router.post('/api/backgrounds/rename', async ({ body }) => {
        await context.safeInvoke('rename_background', {
            dto: {
                old_bg: body?.old_bg || '',
                new_bg: body?.new_bg || '',
            },
        });

        context.invalidateInvokeAll('read_thumbnail_asset');
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
        context.invalidateInvokeAll('read_thumbnail_asset');

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
