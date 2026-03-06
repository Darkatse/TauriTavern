import { t } from './i18n.js';

const DEFAULT_EXPORT_SUCCESS_TIMEOUT = 7000;

function resolveExportDestination(savedPath) {
    const normalizedPath = String(savedPath || '').trim();
    if (!normalizedPath) {
        return '';
    }

    const directory = normalizedPath.replace(/[\\/][^\\/]*$/, '');
    return directory || normalizedPath;
}

export function getExportSuccessMessage(result) {
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
    if (!toastrInstance?.success) {
        return;
    }

    toastrInstance.success(getExportSuccessMessage(result), title, { timeOut });
}
