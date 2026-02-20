import { renderExtensionTemplateAsync } from '../../extensions.js';
import { t } from '../../i18n.js';

const MODULE_NAME = 'data-migration';
const MIGRATED_TARGET_USER = 'default-user';
const JOB_POLL_INTERVAL_MS = 1200;
const TERMINAL_JOB_STATES = new Set(['completed', 'failed', 'cancelled']);

const jobState = {
    jobId: '',
    cancelRequested: false,
};

function extractErrorMessage(text) {
    if (!text) {
        return t`Unknown error`;
    }

    try {
        const json = JSON.parse(text);
        if (typeof json?.error === 'string' && json.error.trim()) {
            return json.error.trim();
        }
        if (typeof json?.message === 'string' && json.message.trim()) {
            return json.message.trim();
        }
    } catch {
        // Ignore JSON parse failure and fallback to plain text.
    }

    return String(text).trim() || t`Unknown error`;
}

async function readFailureMessage(response) {
    const responseText = await response.text();
    return extractErrorMessage(responseText);
}

function normalizeCaughtError(error) {
    if (error instanceof Error && typeof error.message === 'string') {
        return extractErrorMessage(error.message);
    }

    return extractErrorMessage(String(error || ''));
}

function parseJobId(value) {
    const jobId = String(value || '').trim();
    return jobId || '';
}

function isMobileRuntime() {
    if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
        return false;
    }

    return /android|iphone|ipad|ipod/i.test(navigator.userAgent);
}

function isTauriRuntime() {
    return typeof window !== 'undefined'
        && typeof window.__TAURI__ === 'object'
        && typeof window.__TAURI__?.core?.invoke === 'function';
}

function sanitizeExportFileName(value, fallback = 'tauritavern-data.zip') {
    const fileName = String(value || '').trim();
    if (!fileName) {
        return fallback;
    }

    return fileName.replace(/[\\/:*?"<>|]+/g, '_');
}

async function resolveUniqueExportPath(pathApi, existsFn, directory, fileName) {
    const dotIndex = fileName.lastIndexOf('.');
    const hasExtension = dotIndex > 0;
    const baseName = hasExtension ? fileName.slice(0, dotIndex) : fileName;
    const extension = hasExtension ? fileName.slice(dotIndex) : '';

    let suffix = 0;
    while (suffix < 1000) {
        const candidateName = suffix === 0
            ? `${baseName}${extension}`
            : `${baseName} (${suffix})${extension}`;
        const candidatePath = await pathApi.join(directory, candidateName);
        if (!(await existsFn(candidatePath))) {
            return candidatePath;
        }
        suffix += 1;
    }

    throw new Error(t`Unable to allocate export file name in Download folder`);
}

async function saveMobileExportArchive(jobId, archivePath, fileName) {
    const tauri = window.__TAURI__;
    const pathApi = tauri?.path;
    if (typeof pathApi?.downloadDir !== 'function' || typeof pathApi?.join !== 'function') {
        throw new Error(t`Tauri path API is unavailable on mobile`);
    }

    const { copyFile, exists, writeFile } = await import('@tauri-apps/plugin-fs');
    const downloadDirectory = await pathApi.downloadDir();
    if (!downloadDirectory || typeof downloadDirectory !== 'string') {
        throw new Error(t`Unable to resolve Download folder`);
    }

    const normalizedName = sanitizeExportFileName(fileName);
    const destinationPath = await resolveUniqueExportPath(pathApi, exists, downloadDirectory, normalizedName);

    try {
        await copyFile(archivePath, destinationPath);
        return destinationPath;
    } catch (copyError) {
        console.warn('Mobile export direct copy failed, falling back to streamed write:', copyError);
    }

    const response = await fetch(`/api/extensions/data-migration/export/download?id=${encodeURIComponent(jobId)}`, {
        method: 'GET',
        cache: 'no-store',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }
    if (!response.body) {
        throw new Error(t`Export archive stream is unavailable`);
    }

    await writeFile(destinationPath, response.body, {
        create: true,
        truncate: true,
    });

    return destinationPath;
}

function hasActiveJob() {
    return Boolean(jobState.jobId);
}

function setStatusText(message) {
    $('#data_migration_status').text(String(message || ''));
}

function refreshControls() {
    const active = hasActiveJob();
    $('#data_migration_import_button').prop('disabled', active);
    $('#data_migration_export_button').prop('disabled', active);

    const cancelButton = $('#data_migration_cancel_button');
    if (active) {
        cancelButton.show();
        cancelButton.prop('disabled', jobState.cancelRequested);
        return;
    }

    cancelButton.hide();
    cancelButton.prop('disabled', false);
}

function startJobTracking(jobId) {
    jobState.jobId = jobId;
    jobState.cancelRequested = false;
    refreshControls();
}

function stopJobTracking() {
    jobState.jobId = '';
    jobState.cancelRequested = false;
    refreshControls();
}

function onImportButtonClick() {
    if (hasActiveJob()) {
        toastr.warning(t`A migration job is already running`);
        return;
    }

    $('#data_migration_import_input').trigger('click');
}

async function startImportJob(file) {
    const formData = new FormData();
    formData.append('archive', file);

    const response = await fetch('/api/extensions/data-migration/import', {
        method: 'POST',
        body: formData,
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const payload = await response.json();
    const jobId = parseJobId(payload?.job_id);
    if (!jobId) {
        throw new Error(t`Import job id is missing`);
    }

    return jobId;
}

async function startExportJob() {
    const response = await fetch('/api/extensions/data-migration/export', {
        method: 'POST',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const payload = await response.json();
    const jobId = parseJobId(payload?.job_id);
    if (!jobId) {
        throw new Error(t`Export job id is missing`);
    }

    return jobId;
}

async function fetchJobStatus(jobId) {
    const response = await fetch(`/api/extensions/data-migration/job?id=${encodeURIComponent(jobId)}`, {
        method: 'GET',
        cache: 'no-store',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    return response.json();
}

function updateStatusFromJob(status) {
    const stage = String(status?.stage || '').trim();
    const message = String(status?.message || '').trim();
    const progress = Number(status?.progress_percent);

    const parts = [];
    if (stage) {
        parts.push(stage);
    }
    if (Number.isFinite(progress)) {
        parts.push(`${progress.toFixed(1)}%`);
    }
    if (message) {
        parts.push(message);
    }

    if (parts.length === 0) {
        return;
    }

    setStatusText(parts.join(' | '));
}

async function pollUntilTerminal(jobId) {
    while (true) {
        const status = await fetchJobStatus(jobId);
        updateStatusFromJob(status);

        const state = String(status?.state || '').toLowerCase();
        if (TERMINAL_JOB_STATES.has(state)) {
            return status;
        }

        await sleep(JOB_POLL_INTERVAL_MS);
    }
}

function triggerExportDownload(jobId, fileName) {
    const anchor = document.createElement('a');
    anchor.href = `/api/extensions/data-migration/export/download?id=${encodeURIComponent(jobId)}`;
    if (fileName) {
        anchor.download = fileName;
    }

    document.body.append(anchor);
    anchor.click();
    anchor.remove();
}

async function requestCancelActiveJob() {
    if (!hasActiveJob() || jobState.cancelRequested) {
        return;
    }

    jobState.cancelRequested = true;
    refreshControls();

    try {
        const response = await fetch('/api/extensions/data-migration/job/cancel', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ job_id: jobState.jobId }),
        });

        if (!response.ok) {
            const reason = await readFailureMessage(response);
            jobState.cancelRequested = false;
            refreshControls();
            toastr.error(reason, t`Failed to cancel job`);
            return;
        }

        setStatusText(t`Cancellation requested...`);
        toastr.info(t`Cancellation requested`);
    } catch (error) {
        jobState.cancelRequested = false;
        refreshControls();
        toastr.error(normalizeCaughtError(error), t`Failed to cancel job`);
    }
}

async function runMigrationJob(kind, startJob) {
    const failureTitle = kind === 'import' ? t`Data import failed` : t`Data export failed`;

    try {
        const jobId = await startJob();
        startJobTracking(jobId);

        const finalStatus = await pollUntilTerminal(jobId);
        const finalState = String(finalStatus?.state || '').toLowerCase();

        if (finalState === 'completed') {
            if (kind === 'import') {
                const sourceUsers = Array.isArray(finalStatus?.result?.source_users)
                    ? finalStatus.result.source_users.filter((value) => typeof value === 'string' && value.trim())
                    : [];
                const targetUser = String(finalStatus?.result?.target_user || MIGRATED_TARGET_USER);
                const userSummary = sourceUsers.length > 0 ? sourceUsers.join(', ') : t`Unknown`;

                toastr.success(
                    t`Imported users: ${userSummary}. Migrated target: ${targetUser}. Reloading...`,
                    t`Data import completed`,
                    { timeOut: 6000 },
                );
                setStatusText(t`Import completed`);

                setTimeout(() => {
                    location.reload();
                }, 800);
            } else {
                const fileName = String(finalStatus?.result?.file_name || '').trim();
                const archivePath = String(finalStatus?.result?.archive_path || '').trim();
                const useMobileNativeSave = isMobileRuntime() && isTauriRuntime();

                if (useMobileNativeSave) {
                    if (!archivePath) {
                        throw new Error(t`Export archive path is missing`);
                    }

                    setStatusText(t`Saving archive to Download folder...`);
                    const savedPath = await saveMobileExportArchive(jobId, archivePath, fileName);
                    toastr.success(t`Data archive saved: ${savedPath}`, t`Export completed`, { timeOut: 8000 });
                    setStatusText(t`Export completed | ${savedPath}`);
                } else {
                    triggerExportDownload(jobId, fileName);
                    toastr.success(t`Data archive exported`, t`Export completed`);
                    setStatusText(t`Export completed`);
                }
            }
            return;
        }

        if (finalState === 'cancelled') {
            toastr.info(t`Migration job cancelled`);
            setStatusText(t`Job cancelled`);
            return;
        }

        throw new Error(String(finalStatus?.error || t`Unknown error`));
    } catch (error) {
        const failureMessage = normalizeCaughtError(error);
        toastr.error(failureMessage, failureTitle);
        setStatusText(failureMessage);
    } finally {
        stopJobTracking();
    }
}

async function onImportInputChange(event) {
    if (hasActiveJob()) {
        toastr.warning(t`A migration job is already running`);
        return;
    }

    const input = event.currentTarget;
    const file = input?.files?.[0];
    input.value = '';

    if (!file) {
        return;
    }

    const confirmed = window.confirm(
        t`Importing will replace the current local data directory. Continue?`,
    );
    if (!confirmed) {
        return;
    }

    toastr.info(t`Importing data archive...`);
    setStatusText(t`Preparing import...`);
    await runMigrationJob('import', () => startImportJob(file));
}

async function onExportClick() {
    if (hasActiveJob()) {
        toastr.warning(t`A migration job is already running`);
        return;
    }

    toastr.info(t`Exporting data archive...`);
    setStatusText(t`Preparing export...`);
    await runMigrationJob('export', startExportJob);
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

jQuery(async () => {
    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    $('#data_migration_container').append(html);
    refreshControls();

    $('#data_migration_import_button').on('click', onImportButtonClick);
    $('#data_migration_import_input').on('change', onImportInputChange);
    $('#data_migration_export_button').on('click', onExportClick);
    $('#data_migration_cancel_button').on('click', requestCancelActiveJob);
});
