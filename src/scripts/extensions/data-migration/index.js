import { renderExtensionTemplateAsync } from '../../extensions.js';
import { t } from '../../i18n.js';
import {
    isNativeMobileDownloadRuntime,
    writeReadableStreamToMobileDownloadFolder,
} from '../../file-export.js';

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

async function saveMobileExportArchive(jobId, fileName) {
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

    return writeReadableStreamToMobileDownloadFolder(response.body, fileName, {
        fallbackName: 'tauritavern-data.zip',
    });
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
                const useMobileNativeSave = isNativeMobileDownloadRuntime();

                if (useMobileNativeSave) {
                    setStatusText(t`Saving archive to Download folder...`);
                    const savedPath = await saveMobileExportArchive(jobId, fileName);
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
