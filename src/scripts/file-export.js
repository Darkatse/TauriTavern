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

    const staging = createFileStagingService({ invoke });
    const path = await staging.stageBlob(payload, { kind: 'export', preferredName: fileName });
    return invoke('deliver_staged_file', { path, fileName });
}
