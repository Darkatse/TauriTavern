import { createFileStagingService } from '../tauri/main/services/files/file-staging-service.js';

/** @returns {Promise<{ delivered: boolean }>} */
export async function deliverBlob(blob, fileName) {
    const payload = blob instanceof Blob ? blob : new Blob([blob ?? '']);
    const { safeInvoke } = window.__TAURITAVERN__.invoke;
    const staging = createFileStagingService({ safeInvoke });
    const path = await staging.stageBlob(payload, { kind: 'export', preferredName: fileName });
    return safeInvoke('deliver_staged_file', { path, fileName });
}

/**
 * The host downloads and keeps the bytes; the page only names the file from the response type.
 * @param {string} url
 * @param {(mimeType: string) => string} fileNameFor
 * @returns {Promise<{ delivered: boolean }>}
 */
export async function deliverRemoteFile(url, fileNameFor) {
    const { safeInvoke } = window.__TAURITAVERN__.invoke;
    const { downloadId, mimeType } = await safeInvoke('download_remote_file', { url });
    return safeInvoke('deliver_remote_file', { downloadId, fileName: fileNameFor(mimeType ?? '') });
}
