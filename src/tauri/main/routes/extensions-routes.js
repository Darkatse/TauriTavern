import { decodeBase64ToBytes } from '../binary-utils.js';

function sanitizeFileName(value, fallback = 'tauritavern-data.zip') {
    const fileName = String(value || '').trim();
    if (!fileName) {
        return fallback;
    }

    return fileName.replace(/[\\/:*?"<>|]+/g, '_');
}

function parseJobId(value) {
    const jobId = String(value || '').trim();
    return jobId || '';
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
    async function startImportJobFromFileInfo(fileInfo) {
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            return jsonResponse({ error: `Unable to access uploaded archive${reason}` }, 400);
        }

        try {
            const jobId = parseJobId(await context.safeInvoke('start_import_data_archive', {
                archive_path: fileInfo.filePath,
                archive_is_temporary: Boolean(fileInfo.isTemporary),
            }));
            if (!jobId) {
                return jsonResponse({ error: 'Import job id is missing' }, 500);
            }

            return jsonResponse({
                ok: true,
                job_id: jobId,
            });
        } finally {
            await fileInfo.cleanup?.();
        }
    }

    async function loadCompletedExportJobStatus(jobId) {
        const status = await context.safeInvoke('get_data_archive_job_status', {
            job_id: jobId,
        });

        if (status.kind !== 'export') {
            return {
                error: jsonResponse({ error: 'Invalid export job' }, 400),
                status: null,
            };
        }

        if (status.state !== 'completed') {
            return {
                error: jsonResponse({ error: 'Export job is not completed yet' }, 409),
                status: null,
            };
        }

        return {
            error: null,
            status,
        };
    }

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

        return startImportJobFromFileInfo(fileInfo);
    });

    router.post('/api/extensions/data-migration/import/android', async ({ body }) => {
        const contentUri = String(body?.content_uri || '').trim();
        if (!contentUri) {
            return jsonResponse({ error: 'Missing content_uri' }, 400);
        }

        const fileInfo = await context.materializeAndroidContentUriUpload(contentUri);
        return startImportJobFromFileInfo(fileInfo);
    });

    router.post('/api/extensions/data-migration/import/android/pick', async () => {
        const contentUri = await context.pickAndroidImportArchive();
        return jsonResponse({
            ok: true,
            content_uri: String(contentUri),
        });
    });

    router.post('/api/extensions/data-migration/export', async () => {
        const jobId = parseJobId(await context.safeInvoke('start_export_data_archive'));
        if (!jobId) {
            return jsonResponse({ error: 'Export job id is missing' }, 500);
        }
        return jsonResponse({
            ok: true,
            job_id: jobId,
        });
    });

    router.post('/api/extensions/data-migration/export/android/save', async ({ body }) => {
        const jobId = parseJobId(body?.job_id);
        if (!jobId) {
            return jsonResponse({ error: 'Missing job id' }, 400);
        }

        const { error, status } = await loadCompletedExportJobStatus(jobId);
        if (error) {
            return error;
        }

        const archivePath = String(status?.result?.archive_path || '').trim();
        if (!archivePath) {
            return jsonResponse({ error: 'Export archive path is missing' }, 500);
        }

        const saved = await context.saveAndroidExportArchive(
            archivePath,
            String(status?.result?.file_name || 'tauritavern-data.zip'),
        );

        return jsonResponse({
            ok: true,
            saved_target: String(saved?.savedTarget || ''),
        });
    });

    router.get('/api/extensions/data-migration/job', async ({ url }) => {
        const jobId = parseJobId(url?.searchParams?.get('id'));
        if (!jobId) {
            return jsonResponse({ error: 'Missing job id' }, 400);
        }

        const status = await context.safeInvoke('get_data_archive_job_status', {
            job_id: jobId,
        });
        return jsonResponse(status || {});
    });

    router.post('/api/extensions/data-migration/job/cancel', async ({ body }) => {
        const jobId = parseJobId(body?.job_id);
        if (!jobId) {
            return jsonResponse({ error: 'Missing job id' }, 400);
        }

        await context.safeInvoke('cancel_data_archive_job', {
            job_id: jobId,
        });

        return jsonResponse({ ok: true });
    });

    router.get('/api/extensions/data-migration/export/download', async ({ url }) => {
        const jobId = parseJobId(url?.searchParams?.get('id'));
        if (!jobId) {
            return jsonResponse({ error: 'Missing job id' }, 400);
        }

        const { error, status } = await loadCompletedExportJobStatus(jobId);
        if (error) {
            return error;
        }

        const archivePath = status.result.archive_path;

        const payload = await context.safeInvoke('read_data_archive_file', {
            archive_path: archivePath,
        });
        const bytes = Uint8Array.from(payload);

        const headers = new Headers();
        headers.set('Content-Type', 'application/zip');
        headers.set(
            'Content-Disposition',
            `attachment; filename="${sanitizeFileName(status.result.file_name, 'tauritavern-data.zip')}"`,
        );
        headers.set('Cache-Control', 'no-store');
        headers.set('Content-Length', String(bytes.byteLength));

        return new Response(bytes, {
            status: 200,
            headers,
        });
    });

    router.post('/api/extensions/data-migration/export/cleanup', async ({ body }) => {
        const jobId = parseJobId(body?.job_id);
        if (!jobId) {
            return jsonResponse({ error: 'Missing job id' }, 400);
        }

        await context.safeInvoke('cleanup_export_data_archive', {
            job_id: jobId,
        });

        return jsonResponse({ ok: true });
    });

    router.post('/api/extensions/branches', async () => jsonResponse([]));
    router.post('/api/extensions/switch', async () => jsonResponse({ error: 'Branch switching is not supported in Tauri backend' }, 400));
}
