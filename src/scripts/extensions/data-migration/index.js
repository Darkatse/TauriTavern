import { renderExtensionTemplateAsync } from '../../extensions.js';
import { t } from '../../i18n.js';
import { download } from '../../utils.js';

const MODULE_NAME = 'data-migration';

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

function resolveFilenameFromContentDisposition(headerValue) {
    if (!headerValue) {
        return '';
    }

    const matched = /filename\s*=\s*"?([^"]+)"?/i.exec(headerValue);
    return matched?.[1] ? String(matched[1]).trim() : '';
}

async function readFailureMessage(response) {
    const responseText = await response.text();
    return extractErrorMessage(responseText);
}

function onImportButtonClick() {
    $('#data_migration_import_input').trigger('click');
}

async function onImportInputChange(event) {
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
    const formData = new FormData();
    formData.append('archive', file);

    const response = await fetch('/api/extensions/data-migration/import', {
        method: 'POST',
        body: formData,
    });

    if (!response.ok) {
        const reason = await readFailureMessage(response);
        toastr.error(reason, t`Data import failed`);
        return;
    }

    const result = await response.json();
    const sourceUsers = Array.isArray(result?.source_users)
        ? result.source_users.filter((value) => typeof value === 'string' && value.trim())
        : [];
    const userSummary = sourceUsers.length > 0 ? sourceUsers.join(', ') : 'unknown';

    toastr.success(
        t`Imported users: ${userSummary}. Migrated target: default-user. Reloading...`,
        t`Data import completed`,
        { timeOut: 6000 },
    );

    setTimeout(() => {
        location.reload();
    }, 800);
}

async function onExportClick() {
    toastr.info(t`Exporting data archive...`);

    const response = await fetch('/api/extensions/data-migration/export', {
        method: 'POST',
    });

    if (!response.ok) {
        const reason = await readFailureMessage(response);
        toastr.error(reason, t`Data export failed`);
        return;
    }

    const fileNameFromHeader = resolveFilenameFromContentDisposition(
        response.headers.get('Content-Disposition'),
    );
    const fileName = fileNameFromHeader || `tauritavern-data-${Date.now()}.zip`;
    const archiveBlob = await response.blob();
    const archiveBytes = await archiveBlob.arrayBuffer();

    download(archiveBytes, fileName, 'application/zip');
    toastr.success(t`Data archive exported`, t`Export completed`);
}

jQuery(async () => {
    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    $('#data_migration_container').append(html);

    $('#data_migration_import_button').on('click', onImportButtonClick);
    $('#data_migration_import_input').on('change', onImportInputChange);
    $('#data_migration_export_button').on('click', onExportClick);
});
