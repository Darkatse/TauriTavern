function decodeBase64ToBytes(value) {
    const normalized = String(value || '').replace(/\s+/g, '');
    if (!normalized) {
        return new Uint8Array(0);
    }

    const binary = atob(normalized);
    const bytes = new Uint8Array(binary.length);

    for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
    }

    return bytes;
}

function sanitizeFileName(value, fallback = 'tauritavern-data.zip') {
    const fileName = String(value || '').trim();
    if (!fileName) {
        return fallback;
    }

    return fileName.replace(/[\\/:*?"<>|]+/g, '_');
}

function decodeRoutePath(value) {
    try {
        return decodeURIComponent(String(value || ''));
    } catch {
        return String(value || '');
    }
}

function parseThirdPartyAssetRoutePath(wildcard) {
    const decodedPath = decodeRoutePath(wildcard)
        .replace(/[\\/]+/g, '/')
        .replace(/^\/+/, '');
    const segments = decodedPath.split('/').filter(Boolean);
    if (segments.length < 2) {
        return null;
    }

    const extensionFolder = segments[0];
    const relativePath = segments.slice(1).join('/');
    if (!extensionFolder || !relativePath) {
        return null;
    }

    return {
        extensionName: `third-party/${extensionFolder}`,
        relativePath,
    };
}

export function registerExtensionRoutes(router, context, { jsonResponse, textResponse }) {
    router.get('/scripts/extensions/third-party/*', async ({ wildcard }) => {
        const parsed = parseThirdPartyAssetRoutePath(wildcard);
        if (!parsed) {
            return textResponse('Not Found', 404);
        }

        try {
            const payload = await context.safeInvoke('read_third_party_extension_asset', {
                extensionName: parsed.extensionName,
                relativePath: parsed.relativePath,
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
            const message = String(error?.message || error || '').toLowerCase();
            if (message.includes('not found')) {
                return textResponse('Not Found', 404);
            }
            throw error;
        }
    });

    router.all('/api/extensions/discover', async () => {
        const extensions = await context.safeInvoke('get_extensions');
        const mapped = Array.isArray(extensions)
            ? extensions.map((extension) => ({
                name: extension.name,
                type: String(extension.extension_type || 'local').toLowerCase(),
            }))
            : [];

        return jsonResponse(mapped);
    });

    router.post('/api/extensions/install', async ({ body }) => {
        const result = await context.safeInvoke('install_extension', {
            url: body?.url || '',
            global: Boolean(body?.global),
            branch: typeof body?.branch === 'string' && body.branch.trim() ? body.branch.trim() : null,
        });

        return jsonResponse({
            display_name: result?.display_name || body?.url || 'Extension',
            author: result?.author || 'Unknown',
            version: result?.version || '0.0.0',
            extensionPath: result?.extension_path || '',
        });
    });

    router.post('/api/extensions/update', async ({ body }) => {
        const result = await context.safeInvoke('update_extension', {
            extensionName: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({
            isUpToDate: Boolean(result?.is_up_to_date),
            shortCommitHash: result?.short_commit_hash || 'unknown',
        });
    });

    router.post('/api/extensions/delete', async ({ body }) => {
        await context.safeInvoke('delete_extension', {
            extensionName: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/extensions/version', async ({ body }) => {
        const result = await context.safeInvoke('get_extension_version', {
            extensionName: body?.extensionName || '',
            global: Boolean(body?.global),
        });

        return jsonResponse({
            currentBranchName: result?.current_branch_name || 'main',
            currentCommitHash: result?.current_commit_hash || 'unknown',
            isUpToDate: Boolean(result?.is_up_to_date),
            remoteUrl: result?.remote_url || '',
        });
    });

    router.post('/api/extensions/move', async ({ body }) => {
        await context.safeInvoke('move_extension', {
            extensionName: body?.extensionName || '',
            source: body?.source || 'local',
            destination: body?.destination || 'global',
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/extensions/data-migration/import', async ({ body }) => {
        if (!(body instanceof FormData)) {
            return jsonResponse({ error: 'Expected multipart form data' }, 400);
        }

        const archive = body.get('archive');
        if (!(archive instanceof Blob)) {
            return jsonResponse({ error: 'No data archive provided' }, 400);
        }

        const preferredName = archive instanceof File && archive.name
            ? archive.name
            : 'data.zip';
        const fileInfo = await context.materializeUploadFile(archive, {
            preferredName,
            preferredExtension: 'zip',
        });

        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded archive${reason}` }, 400);
        }

        try {
            const result = await context.safeInvoke('import_data_archive', {
                archive_path: fileInfo.filePath,
            });

            await Promise.allSettled([
                context.safeInvoke('clear_character_cache'),
                context.safeInvoke('clear_chat_cache'),
                context.safeInvoke('clear_group_cache'),
            ]);

            return jsonResponse({
                ok: true,
                source_users: Array.isArray(result?.source_users) ? result.source_users : [],
                target_user: result?.target_user || 'default-user',
            });
        } finally {
            await fileInfo.cleanup?.();
        }
    });

    router.post('/api/extensions/data-migration/export', async () => {
        const result = await context.safeInvoke('export_data_archive');
        const encodedArchive = String(result?.zip_base64 || '');
        if (!encodedArchive) {
            return jsonResponse({ error: 'Exported archive is empty' }, 500);
        }

        const bytes = decodeBase64ToBytes(encodedArchive);
        const fileName = sanitizeFileName(result?.file_name, 'tauritavern-data.zip');

        return new Response(bytes, {
            status: 200,
            headers: {
                'Content-Type': 'application/zip',
                'Content-Disposition': `attachment; filename="${fileName}"`,
            },
        });
    });

    router.post('/api/extensions/branches', async () => jsonResponse([]));
    router.post('/api/extensions/switch', async () => jsonResponse({ error: 'Branch switching is not supported in Tauri backend' }, 400));
}
