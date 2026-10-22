import { commitBytes } from '../services/files/byte-commit.js';
import { textFragmentsToByteChunks } from '../kernel/utf8.js';

export function registerWorldInfoRoutes(router, context, { jsonResponse }) {
    function jsonBytes(bytes) {
        if (!(bytes instanceof ArrayBuffer) && !ArrayBuffer.isView(bytes)) {
            throw new Error('Host returned a non-binary world info response');
        }
        return new Response(bytes, { headers: { 'Content-Type': 'application/json' } });
    }

    router.post('/api/worldinfo/get', async ({ body }) => {
        const name = typeof body?.name === 'string' ? body.name : '';
        if (name === '') {
            return jsonResponse({ error: 'World file must have a name' }, 400);
        }

        const bytes = await context.safeInvoke('get_world_info', { dto: { name } });
        return jsonBytes(bytes);
    });

    router.post('/api/worldinfo/sanitize-name', async ({ body }) => {
        const name = typeof body?.name === 'string' ? body.name : '';
        if (name === '') {
            return jsonResponse({ error: 'World file must have a name' }, 400);
        }

        const result = await context.safeInvoke('normalize_world_info_name', {
            dto: {
                name,
                import_filename: Boolean(body?.importFilename),
            },
        });

        return jsonResponse(result || {});
    });

    router.post('/api/worldinfo/edit', async ({ body }) => {
        // Fetch already supplies a JSON snapshot; jQuery may supply an object instead.
        const text = typeof body === 'string' ? body : JSON.stringify(body);
        if (typeof text !== 'string') {
            return jsonResponse({ error: 'World info replacement requires JSON' }, 400);
        }
        await commitBytes({
            begin: () => context.invokeTransport('begin_world_info_commit'),
            frames: maxBytes => textFragmentsToByteChunks([text], maxBytes),
            append: (data, options) => context.invokeTransport('append_world_info_commit_chunk', data, options),
            finish: (sessionId, expectedSize) => context.invokeTransport('finish_world_info_commit', {
                sessionId,
                expectedSize,
            }),
            abort: sessionId => context.invokeTransport('abort_world_info_commit', { sessionId }),
        });
        return jsonResponse({ ok: true });
    }, { body: 'text' });

    router.post('/api/worldinfo/delete', async ({ body }) => {
        const name = typeof body?.name === 'string' ? body.name : '';
        if (name === '') {
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

        const fileInfo = await context.materializeUploadFile(file, {
            kind: 'worldinfo-import',
            preferredName: originalFilename,
        });
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded world info file path${reason}` }, 400);
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
