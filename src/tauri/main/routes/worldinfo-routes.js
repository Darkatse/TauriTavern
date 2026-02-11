export function registerWorldInfoRoutes(router, context, { jsonResponse }) {
    router.post('/api/worldinfo/get', async ({ body }) => {
        const name = String(body?.name || '').trim();
        if (!name) {
            return jsonResponse({ error: 'World file must have a name' }, 400);
        }

        const worldInfo = await context.safeInvoke('get_world_info', {
            dto: { name },
        });

        return jsonResponse(worldInfo || { entries: {} });
    });

    router.post('/api/worldinfo/edit', async ({ body }) => {
        const name = String(body?.name || '').trim();
        const data = body?.data;

        if (!name) {
            return jsonResponse({ error: 'World file must have a name' }, 400);
        }

        if (!data || typeof data !== 'object' || Array.isArray(data)) {
            return jsonResponse({ error: 'Is not a valid world info file' }, 400);
        }

        await context.safeInvoke('save_world_info', {
            dto: {
                name,
                data,
            },
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/worldinfo/delete', async ({ body }) => {
        const name = String(body?.name || '').trim();
        if (!name) {
            return jsonResponse({ error: 'World file must have a name' }, 400);
        }

        await context.safeInvoke('delete_world_info', {
            dto: { name },
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/worldinfo/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const file = body.get('avatar');
        if (!(file instanceof Blob)) {
            return jsonResponse({ error: 'No world info file provided' }, 400);
        }

        const convertedDataRaw = body.get('convertedData');
        const convertedData = convertedDataRaw == null ? null : String(convertedDataRaw);
        const originalFilename = file instanceof File ? file.name : 'world-info.json';

        // When convertedData is already provided by frontend, importing can be fully in-memory.
        if (convertedData && convertedData.trim().length > 0) {
            const result = await context.safeInvoke('import_world_info', {
                dto: {
                    file_path: '',
                    original_filename: originalFilename,
                    converted_data: convertedData,
                },
            });

            return jsonResponse(result || {});
        }

        const fileInfo = await context.materializeUploadFile(file);
        if (!fileInfo?.filePath) {
            return jsonResponse({ error: 'Unable to access uploaded world info file path' }, 400);
        }

        try {
            const result = await context.safeInvoke('import_world_info', {
                dto: {
                    file_path: fileInfo.filePath,
                    original_filename: originalFilename,
                    converted_data: null,
                },
            });

            return jsonResponse(result || {});
        } finally {
            await fileInfo.cleanup?.();
        }
    });
}
