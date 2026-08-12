function formText(body, name) {
    const value = body.get(name);
    return value == null ? '' : String(value);
}

async function invokeWithUpload(context, file, options, command, dto) {
    const fileInfo = await context.materializeUploadFile(file, options);
    if (!fileInfo?.filePath) {
        const detail = fileInfo?.error ? `: ${fileInfo.error}` : '';
        throw new Error(`Bad request: Unable to access uploaded file${detail}`);
    }

    try {
        return await context.safeInvoke(command, {
            dto: { ...dto, file_path: fileInfo.filePath },
        });
    } finally {
        await fileInfo.cleanup?.();
    }
}

export function registerSpriteRoutes(router, context, { jsonResponse, textResponse }) {
    router.get('/api/sprites/get', async ({ url }) => {
        const name = String(url.searchParams.get('name') || '');
        if (!name) {
            return textResponse('Bad Request', 400);
        }

        const sprites = await context.safeInvoke('list_sprites', { dto: { name } });
        return jsonResponse(sprites || []);
    });

    router.post('/api/sprites/upload', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return textResponse('Bad Request', 400);
        }

        const file = body.get('avatar');
        const name = formText(body, 'name');
        const label = formText(body, 'label');
        const spriteName = formText(body, 'spriteName') || label;
        if (!(file instanceof Blob) || !name || !label) {
            return textResponse('Bad Request', 400);
        }

        const originalFilename = file instanceof File && file.name ? file.name : 'sprite.bin';
        await invokeWithUpload(
            context,
            file,
            { kind: 'sprite', preferredName: originalFilename },
            'upload_sprite',
            {
                name,
                sprite_name: spriteName,
                original_filename: originalFilename,
            },
        );
        return jsonResponse({ ok: true });
    });

    router.post('/api/sprites/upload-zip', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return textResponse('Bad Request', 400);
        }

        const file = body.get('avatar');
        const name = formText(body, 'name');
        if (!(file instanceof Blob) || !name) {
            return textResponse('Bad Request', 400);
        }

        const originalFilename = file instanceof File && file.name ? file.name : 'sprites.zip';
        const count = await invokeWithUpload(
            context,
            file,
            { kind: 'sprite-pack', preferredName: originalFilename, preferredExtension: 'zip' },
            'upload_sprite_pack',
            { name },
        );
        return jsonResponse({ ok: true, count: Number(count) || 0 });
    });

    router.post('/api/sprites/delete', async ({ body }) => {
        const name = String(body?.name || '');
        const spriteName = String(body?.spriteName || body?.label || '');
        if (!name || !spriteName) {
            return textResponse('Bad Request', 400);
        }

        await context.safeInvoke('delete_sprite', {
            dto: { name, sprite_name: spriteName },
        });
        return textResponse('OK');
    });
}
