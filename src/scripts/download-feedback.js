import { t } from './i18n.js';

const DEFAULT_EXPORT_SUCCESS_TIMEOUT = 7000;
const DEFAULT_EXPORT_FAILURE_TIMEOUT = 10000;

function isIosNativeShareResult(result) {
    return result?.mode === 'ios-native-share';
}

function resolveExportDestination(savedPath) {
    const normalizedPath = String(savedPath || '').trim();
    if (!normalizedPath) {
        return '';
    }

    const directory = normalizedPath.replace(/[\\/][^\\/]*$/, '');
    return directory || normalizedPath;
}

export function getExportSuccessMessage(result) {
    if (isIosNativeShareResult(result)) {
        return t`Export completed.`;
    }

    const destination = resolveExportDestination(result?.savedPath);
    return destination
        ? t`Exported to: ${destination}`
        : t`Export started. Check your default download folder.`;
}

export function showExportSuccessToast(
    result,
    {
        toastrInstance = globalThis.toastr,
        title = t`Export completed`,
        timeOut = DEFAULT_EXPORT_SUCCESS_TIMEOUT,
    } = {},
) {
    if (isIosNativeShareResult(result) && result?.completed !== true) {
        return;
    }

    if (!toastrInstance?.success) {
        return;
    }

    toastrInstance.success(getExportSuccessMessage(result), title, { timeOut });
}

function resolveExportFailureMessage(error) {
    if (typeof error === 'string' && error.trim()) {
        return error.trim();
    }

    if (typeof error?.message === 'string' && error.message.trim()) {
        return error.message.trim();
    }

    return t`Failed to export file.`;
}

export function showExportFailureToast(
    error,
    {
        toastrInstance = globalThis.toastr,
        title = t`Export failed`,
        timeOut = DEFAULT_EXPORT_FAILURE_TIMEOUT,
    } = {},
) {
    if (!toastrInstance?.error) {
        return;
    }

    toastrInstance.error(resolveExportFailureMessage(error), title, { timeOut });
}
