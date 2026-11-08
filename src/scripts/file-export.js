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
