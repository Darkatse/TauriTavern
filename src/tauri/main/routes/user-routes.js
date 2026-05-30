import { isAndroidRuntime, isIosRuntime } from '../../../scripts/util/mobile-runtime.js';
import { sanitizeAttachmentFileName } from '../binary-utils.js';
import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

async function cleanupUserBackupArchive(context, archivePath) {
    const normalizedPath = String(archivePath || '').trim();
    if (!normalizedPath) {
        return null;
    }

    try {
        await context.safeInvoke('cleanup_user_backup_archive', {
            archive_path: normalizedPath,
        });
        return null;
    } catch (error) {
        return extractErrorText(error) || 'Failed to cleanup user backup archive';
    }
}

async function createUserBackupArchiveStream(context, archivePath) {
    if (typeof context.createReadableFileStream !== 'function') {
        throw new Error('Readable file stream service is unavailable');
    }

    const sourceStream = await context.createReadableFileStream(archivePath);
    const reader = sourceStream.getReader();
    let cleaned = false;

    async function cleanupOnce() {
        if (cleaned) {
            return;
        }

        cleaned = true;
        const cleanupError = await cleanupUserBackupArchive(context, archivePath);
        if (cleanupError) {
            console.warn('Failed to cleanup user backup archive after streaming:', cleanupError);
        }
    }

    return new ReadableStream({
        async pull(controller) {
            try {
                const { done, value } = await reader.read();
                if (done) {
                    await cleanupOnce();
                    controller.close();
                    return;
                }
                controller.enqueue(value);
            } catch (error) {
                await cleanupOnce();
                throw error;
            }
        },
        async cancel(reason) {
            try {
                await reader.cancel(reason);
            } finally {
                await cleanupOnce();
            }
        },
    });
}

export function registerUserRoutes(router, context, { jsonResponse }) {
    router.post('/api/users/backup', async ({ body }) => {
        const handle = String(body?.handle || '').trim();
        if (!handle) {
            return jsonResponse({ error: 'Bad request: User handle is required for backup' }, 400);
        }
        const useNativeSave = body?.native === true;

        let archivePath = '';

        try {
            const secretSettings = await context.safeInvoke('read_secret_settings');
            const includeSecrets = secretSettings?.allowKeysExposure === true;
            const archive = await context.safeInvoke('export_user_backup_archive', {
                handle,
                include_secrets: includeSecrets,
            });
            const archiveFileName = String(archive?.file_name || '').trim();
            if (!archiveFileName) {
                throw new Error('Internal server error: User backup archive filename is missing');
            }

            archivePath = String(archive?.archive_path || '').trim();
            if (!archivePath) {
                throw new Error('Internal server error: User backup archive path is missing');
            }

            const fileName = sanitizeAttachmentFileName(archiveFileName, `${handle}.zip`);

            if (!useNativeSave) {
                const stream = await createUserBackupArchiveStream(context, archivePath);
                const response = new Response(stream, {
                    status: 200,
                    headers: {
                        'Content-Type': 'application/zip',
                        'Content-Disposition': `attachment; filename="${encodeURI(fileName)}"`,
                    },
                });
                archivePath = '';
                return response;
            }

            if (isAndroidRuntime()) {
                let cleanupError = null;
                try {
                    const saved = await context.saveAndroidExportArchive(archivePath, fileName);
                    cleanupError = await cleanupUserBackupArchive(context, archivePath);
                    archivePath = '';
                    return jsonResponse({
                        ok: true,
                        mode: 'mobile-native',
                        file_name: fileName,
                        saved_target: String(saved?.savedTarget || ''),
                        includes_secrets: includeSecrets,
                        cleanup_error: cleanupError,
                    });
                } catch (error) {
                    cleanupError = await cleanupUserBackupArchive(context, archivePath);
                    archivePath = '';
                    if (cleanupError) {
                        console.warn('Failed to cleanup user backup archive after Android save error:', cleanupError);
                    }
                    throw error;
                }
            }

            if (isIosRuntime()) {
                const result = await context.safeInvoke('ios_share_file', {
                    file_path: archivePath,
                });
                const cleanupError = await cleanupUserBackupArchive(context, archivePath);

                return jsonResponse({
                    ok: true,
                    mode: 'ios-native-share',
                    file_name: fileName,
                    completed: Boolean(result?.completed),
                    activity: result?.activity ? String(result.activity) : null,
                    includes_secrets: includeSecrets,
                    cleanup_error: cleanupError,
                });
            }

            const savedTarget = await context.safeInvoke('save_user_backup_archive', {
                archive_path: archivePath,
                file_name: fileName,
            });

            return jsonResponse({
                ok: true,
                mode: 'desktop-native',
                file_name: fileName,
                saved_target: String(savedTarget || ''),
                includes_secrets: includeSecrets,
            });
        } catch (error) {
            const cleanupError = await cleanupUserBackupArchive(context, archivePath);
            if (cleanupError) {
                console.warn('Failed to cleanup user backup archive after backup error:', cleanupError);
            }
            const resolved = resolveHostErrorResponse(extractErrorText(error));
            return jsonResponse({ error: resolved.body }, resolved.status);
        }
    });
}
