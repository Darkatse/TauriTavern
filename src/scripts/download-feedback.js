import { t } from './i18n.js';

const DEFAULT_EXPORT_SUCCESS_TIMEOUT = 7000;
const DEFAULT_EXPORT_FAILURE_TIMEOUT = 10000;

export function getExportSuccessMessage() {
    return t`Export completed.`;
}

export function showExportSuccessToast(
    result,
    {
        toastrInstance = globalThis.toastr,
        title = t`Export completed`,
        timeOut = DEFAULT_EXPORT_SUCCESS_TIMEOUT,
    } = {},
) {
    if (result?.delivered !== true) {
        return;
    }

    if (!toastrInstance?.success) {
        return;
    }

    toastrInstance.success(getExportSuccessMessage(), title, { timeOut });
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
