import { convertFileSrc } from '../tauri-bridge.js';

function sanitizeFileName(fileName) {
    const fallback = 'shared-character.png';
    const raw = String(fileName || '').trim();
    const sanitized = raw
        .replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '_')
        .replace(/[. ]+$/g, '')
        .trim();

    if (!sanitized) {
        return fallback;
    }

    return sanitized.toLowerCase().endsWith('.png') ? sanitized : `${sanitized}.png`;
}

async function fileFromSharedPath(payload) {
    const response = await fetch(convertFileSrc(payload.path, 'asset'));
    if (!response.ok) {
        throw new Error(`Failed to read shared PNG (HTTP ${response.status})`);
    }

    const blob = await response.blob();
    return new File([blob], sanitizeFileName(payload.fileName), { type: payload.mimeType });
}

// The native share bridge delivers payloads it has already normalized.
async function handleNativeSharePayload(payload, { importFromExternalUrl, processDroppedFiles }) {
    switch (payload.kind) {
        case 'url':
            await importFromExternalUrl(payload.url);
            return;
        case 'png':
            await processDroppedFiles([await fileFromSharedPath(payload)]);
            return;
        default:
            throw new Error(`Unsupported native share payload: ${payload.kind}`);
    }
}

export function initializeShareTargetImport({ importFromExternalUrl, processDroppedFiles }) {
    let processingQueue = Promise.resolve();
    const enqueue = (payload) => {
        processingQueue = processingQueue
            .then(() => handleNativeSharePayload(payload, { importFromExternalUrl, processDroppedFiles }))
            .catch((error) => {
                console.error('Failed to import shared payload:', error);
            });
    };

    return window.__TAURITAVERN_NATIVE_SHARE__.subscribe(enqueue);
}
