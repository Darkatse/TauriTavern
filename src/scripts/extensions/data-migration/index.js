import { renderExtensionTemplateAsync } from '../../extensions.js';
import { t, translate } from '../../i18n.js';
import { Popup } from '../../popup.js';
import { getActiveIosPolicyActivationReport } from '../../tauritavern/ios-policy.js';
import { flushLifecycleState } from '../../../tauri/main/services/lifecycle/lifecycle-flush-service.js';

const MODULE_NAME = 'data-migration';
const JOB_POLL_INTERVAL_MS = 1200;
const TERMINAL_JOB_STATES = new Set(['completed', 'failed', 'cancelled']);

const SILLYTAVERN_MIGRATION_COPY_KEY = 'Import a SillyTavern data archive (zip, tar, tar.gz, or tgz) and migrate it to TauriTavern.';
const TAURITAVERN_MIGRATION_COPY_KEY = 'Import a TauriTavern data zip archive from another device and migrate it to this TauriTavern.';

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

async function startImportJob() {
    const response = await fetch('/api/extensions/data-migration/import', { method: 'POST' });
    if (!response.ok) throw new Error(await readFailureMessage(response));
    const payload = await response.json();
    return payload.cancelled ? null : requireJobId(payload, t`Import job id is missing`);
}

async function saveExportArchive(jobId) {
    const response = await fetch('/api/extensions/data-migration/export/save', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ job_id: jobId }),
    });
    if (!response.ok) throw new Error(await readFailureMessage(response));
    return response.json();
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
    try {
        if (hasActiveJob()) {
            toastr.warning(t`A migration job is already running`);
            return;
        }

        await runConfirmedImport();
    } catch (error) {
        const failureMessage = normalizeCaughtError(error);
        toastr.error(failureMessage, t`Data import failed`);
        setStatusText(failureMessage);
    }
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

function importStatusRequiresReload(status) {
    return Boolean(status?.local_applied) || Boolean(status?.reconcile_error);
}

function importTerminalMessage(status, fallback) {
    const message = String(status?.error || fallback || '').trim();
    const reconcileError = String(status?.reconcile_error || '').trim();
    return [message, reconcileError].filter(Boolean).join(' | ');
}

function reloadSoon() {
    setTimeout(() => {
        location.reload();
    }, 800);
}

async function requestCancelActiveJob() {
    if (!jobState.jobId || jobState.cancelRequested) {
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
        markJobStarting();
        await flushLifecycleState(`data-archive:${kind}`);
        if (kind === 'import') {
            setStatusText(t`Preparing import...`);
        }
        const jobId = await startJob();
        if (!jobId) {
            if (kind === 'import') {
                toastr.info(t`Import cancelled`);
                setStatusText(t`Import cancelled`);
                return;
            }

            throw new Error(t`Migration job did not return a job id`);
        }
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

                reloadSoon();
            } else {
                const saveResult = await saveExportArchive(jobId);
                if (saveResult.delivered) {
                    toastr.success(t`Data archive exported`, t`Export completed`);
                    setStatusText(t`Export completed`);
                } else {
                    setStatusText(t`Export cancelled`);
                }
            }
            return;
        }

        if (finalState === 'cancelled') {
            if (kind === 'import' && importStatusRequiresReload(finalStatus)) {
                const message = importTerminalMessage(finalStatus, t`Import cancelled after updating local data`);
                toastr.warning(`${message}. ${t`Reloading...`}`, t`Data import cancelled`, { timeOut: 6000 });
                setStatusText(message);
                reloadSoon();
                return;
            }

            toastr.info(t`Migration job cancelled`);
            setStatusText(t`Job cancelled`);
            return;
        }

        if (kind === 'import' && importStatusRequiresReload(finalStatus)) {
            const message = importTerminalMessage(finalStatus, t`Data import failed after updating local data`);
            toastr.error(`${message}. ${t`Reloading...`}`, failureTitle, { timeOut: 8000 });
            setStatusText(message);
            reloadSoon();
            return;
        }

        throw new Error(finalStatus.error || t`Unknown error`);
    } catch (error) {
        const failureMessage = normalizeCaughtError(error);
        toastr.error(failureMessage, failureTitle);
        setStatusText(failureMessage);
    } finally {
        stopJobTracking();
    }
}

async function runConfirmedImport() {
    const prompt = t`Importing will merge into the current local data directory (same-path files will be overwritten). Continue?`;
    if (!await Popup.show.confirm(t`Confirm data import`, prompt)) return;
    toastr.info(t`Importing data archive...`);
    await runMigrationJob('import', startImportJob);
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

    const iosPolicy = getActiveIosPolicyActivationReport();
    if (iosPolicy?.profile === 'ios_external_beta') {
        const description = document.querySelector(`#data_migration_settings .extensions_info[data-i18n="${CSS.escape(SILLYTAVERN_MIGRATION_COPY_KEY)}"]`);
        if (!(description instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] Data migration description element not found');
        }

        description.dataset.i18n = TAURITAVERN_MIGRATION_COPY_KEY;
        description.textContent = translate(TAURITAVERN_MIGRATION_COPY_KEY);
    }

    $('#data_migration_import_button').on('click', onImportButtonClick);
    $('#data_migration_export_button').on('click', onExportClick);
    $('#data_migration_cancel_button').on('click', requestCancelActiveJob);
});
