import { renderExtensionTemplateAsync } from '../../extensions.js';
import { t } from '../../i18n.js';
import { downloadBlobWithRuntime } from '../../file-export.js';

const MODULE_NAME = 'data-migration';
const JOB_POLL_INTERVAL_MS = 1200;
const TERMINAL_JOB_STATES = new Set(['completed', 'failed', 'cancelled']);
const PREPARING_PROGRESS_INTERVAL_MS = 320;
const PREPARING_PROGRESS_MAX_BEFORE_JOB = 96;

const jobState = {
    jobId: '',
    starting: false,
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

function requireJobId(payload, errorMessage) {
    if (typeof payload?.job_id !== 'string' || !payload.job_id.trim()) {
        throw new Error(errorMessage);
    }

    return payload.job_id.trim();
}

function isAndroidRuntime() {
    if (typeof navigator === 'undefined' || typeof navigator.userAgent !== 'string') {
        return false;
    }

    return /android/i.test(navigator.userAgent);
}

async function requestImportJob(url, init) {
    const response = await fetch(url, init);
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const payload = await response.json();
    return requireJobId(payload, t`Import job id is missing`);
}

async function startImportJobFromMultipart(file) {
    const formData = new FormData();
    formData.append('archive', file);
    return requestImportJob('/api/extensions/data-migration/import', {
        method: 'POST',
        body: formData,
    });
}

async function startImportJobFromAndroidContentUri(contentUri) {
    return requestImportJob('/api/extensions/data-migration/import/android', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({
            content_uri: contentUri,
        }),
    });
}

async function pickAndroidImportArchive() {
    const response = await fetch('/api/extensions/data-migration/import/android/pick', {
        method: 'POST',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const payload = await response.json();
    const contentUri = String(payload?.content_uri || '').trim();
    if (!contentUri) {
        throw new Error(t`Android import picker did not return a content URI`);
    }

    return contentUri;
}

async function downloadExportArchive(jobId, fileName) {
    if (isAndroidRuntime()) {
        const response = await fetch('/api/extensions/data-migration/export/android/save', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ job_id: jobId }),
        });
        if (!response.ok) {
            throw new Error(await readFailureMessage(response));
        }

        const payload = await response.json();
        return {
            mode: 'mobile-native',
            savedPath: String(payload?.saved_target || ''),
        };
    }

    const response = await fetch(`/api/extensions/data-migration/export/download?id=${encodeURIComponent(jobId)}`, {
        method: 'GET',
        cache: 'no-store',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const blob = await response.blob();
    return downloadBlobWithRuntime(blob, fileName, { fallbackName: 'tauritavern-data.zip' });
}

async function cleanupExportArchive(jobId) {
    const response = await fetch('/api/extensions/data-migration/export/cleanup', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({ job_id: jobId }),
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }
}

function hasActiveJob() {
    return jobState.starting || Boolean(jobState.jobId);
}

function setStatusText(message) {
    $('#data_migration_status').text(String(message || ''));
}

function refreshControls() {
    const busy = hasActiveJob();
    $('#data_migration_import_button').prop('disabled', busy);
    $('#data_migration_export_button').prop('disabled', busy);

    const cancelButton = $('#data_migration_cancel_button');
    if (jobState.jobId) {
        cancelButton.show();
        cancelButton.prop('disabled', jobState.cancelRequested);
        return;
    }

    cancelButton.hide();
    cancelButton.prop('disabled', false);
}

function markJobStarting() {
    jobState.jobId = '';
    jobState.starting = true;
    jobState.cancelRequested = false;
    refreshControls();
}

function startJobTracking(jobId) {
    jobState.jobId = jobId;
    jobState.starting = false;
    jobState.cancelRequested = false;
    refreshControls();
}

function stopJobTracking() {
    jobState.jobId = '';
    jobState.starting = false;
    jobState.cancelRequested = false;
    refreshControls();
}

async function onImportButtonClick() {
    if (hasActiveJob()) {
        toastr.warning(t`A migration job is already running`);
        return;
    }

    if (isAndroidRuntime()) {
        await onAndroidImportButtonClick();
        return;
    }

    $('#data_migration_import_input').trigger('click');
}

async function startExportJob() {
    const response = await fetch('/api/extensions/data-migration/export', {
        method: 'POST',
    });
    if (!response.ok) {
        throw new Error(await readFailureMessage(response));
    }

    const payload = await response.json();
    return requireJobId(payload, t`Export job id is missing`);
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

        const state = status.state;
        if (TERMINAL_JOB_STATES.has(state)) {
            return status;
        }

        await sleep(JOB_POLL_INTERVAL_MS);
    }
}

async function requestCancelActiveJob() {
    if (!hasRunningJob() || jobState.cancelRequested) {
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

function createPreparingImportProgress() {
    let progress = 0;
    let timerId = null;

    const render = () => {
        setStatusText(`${t`Preparing import...`} ${progress}%`);
    };

    return {
        start() {
            progress = 0;
            render();

            timerId = window.setInterval(() => {
                const step = progress < 60 ? 4 : progress < 85 ? 2 : 1;
                progress = Math.min(PREPARING_PROGRESS_MAX_BEFORE_JOB, progress + step);
                render();
            }, PREPARING_PROGRESS_INTERVAL_MS);
        },
        complete() {
            if (timerId !== null) {
                clearInterval(timerId);
                timerId = null;
            }
            progress = 100;
            render();
        },
        stop() {
            if (timerId !== null) {
                clearInterval(timerId);
                timerId = null;
            }
        },
    };
}

async function runMigrationJob(kind, startJob) {
    const failureTitle = kind === 'import' ? t`Data import failed` : t`Data export failed`;
    const preparingProgress = kind === 'import' ? createPreparingImportProgress() : null;

    try {
        markJobStarting();
        preparingProgress?.start();
        const jobId = await startJob();
        preparingProgress?.complete();
        startJobTracking(jobId);

        const finalStatus = await pollUntilTerminal(jobId);
        const finalState = finalStatus.state;

        if (finalState === 'completed') {
            if (kind === 'import') {
                const sourceUsers = finalStatus.result.source_users;
                const targetUser = finalStatus.result.target_user;
                const userSummary = sourceUsers.join(', ');

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
                const fileName = finalStatus.result.file_name;
                const downloadResult = await downloadExportArchive(jobId, fileName);
                void cleanupExportArchive(jobId).catch((error) => {
                    console.warn('Failed to cleanup export archive:', error);
                });

                if (downloadResult.mode === 'mobile-native') {
                    const savedPath = downloadResult.savedPath;
                    toastr.success(t`Data archive saved: ${savedPath}`, t`Export completed`, { timeOut: 8000 });
                    setStatusText(t`Export completed | ${savedPath}`);
                } else {
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

        throw new Error(finalStatus.error || t`Unknown error`);
    } catch (error) {
        const failureMessage = normalizeCaughtError(error);
        toastr.error(failureMessage, failureTitle);
        setStatusText(failureMessage);
    } finally {
        preparingProgress?.stop();
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

    await runConfirmedImport(() => startImportJobFromMultipart(file));
}

async function onAndroidImportButtonClick() {
    await runConfirmedImport(async () => {
        const contentUri = await pickAndroidImportArchive();
        return startImportJobFromAndroidContentUri(contentUri);
    });
}

async function runConfirmedImport(startJob) {
    const confirmed = window.confirm(
        t`Importing will replace the current local data directory. Continue?`,
    );
    if (!confirmed) {
        return;
    }

    toastr.info(t`Importing data archive...`);
    await runMigrationJob('import', startJob);
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
