import { createFileStagingService } from '../tauri/main/services/files/file-staging-service.js';

/** @returns {Promise<{ delivered: boolean }>} */
export async function deliverBlob(blob, fileName) {
    const payload = blob instanceof Blob ? blob : new Blob([blob ?? '']);
    const invoke = window.__TAURI__?.core?.invoke;
    if (typeof invoke !== 'function') {
        const objectUrl = URL.createObjectURL(payload);
        const anchor = document.createElement('a');
        anchor.href = objectUrl;
        anchor.download = fileName || 'download.bin';
        document.body.append(anchor);
        anchor.click();
        anchor.remove();
        setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
        return { delivered: true };
    }

    const { safeInvoke } = window.__TAURITAVERN__.invoke;
    const staging = createFileStagingService({ safeInvoke, invoke });
    const path = await staging.stageBlob(payload, { kind: 'export', preferredName: fileName });
    return safeInvoke('deliver_staged_file', { path, fileName });
}

/**
 * Stages remote bytes in the host, then names the file using the response type.
 * @param {string} url
 * @param {(mimeType: string) => string} fileNameFor
 * @returns {Promise<{ delivered: boolean }>}
 */
export async function deliverRemoteFile(url, fileNameFor) {
    const { safeInvoke } = window.__TAURITAVERN__.invoke;
    const { path, mimeType } = await createFileStagingService({ safeInvoke }).stageUrl(url);
    return safeInvoke('deliver_staged_file', { path, fileName: fileNameFor(mimeType) });
}
