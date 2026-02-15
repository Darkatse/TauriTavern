import { convertFileSrc, isTauriEnv } from '../tauri-bridge.js';

const NATIVE_SHARE_BRIDGE_KEY = '__TAURITAVERN_NATIVE_SHARE__';

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

function getNativeShareBridge() {
    if (typeof window === 'undefined') {
        return null;
    }

    const bridge = window[NATIVE_SHARE_BRIDGE_KEY];
    if (!bridge || typeof bridge.subscribe !== 'function') {
        return null;
    }

    return bridge;
}

function normalizeHttpUrl(url) {
    try {
        const parsed = new URL(String(url || '').trim());
        const protocol = parsed.protocol.toLowerCase();
        if (protocol !== 'http:' && protocol !== 'https:') {
            return null;
        }
        return parsed.toString();
    } catch {
        return null;
    }
}

async function fileFromSharedPath(payload) {
    const sourcePath = String(payload?.path || '').trim();
    if (!sourcePath) {
        return null;
    }

    if (typeof convertFileSrc !== 'function') {
        throw new Error('Tauri convertFileSrc is unavailable');
    }

    const sourceUrl = convertFileSrc(sourcePath, 'asset');
    const response = await fetch(sourceUrl);
    if (!response.ok) {
        throw new Error(`Failed to read shared PNG (HTTP ${response.status})`);
    }

    const blob = await response.blob();
    const fileName = sanitizeFileName(payload?.fileName);
    const mimeType = String(payload?.mimeType || blob.type || 'image/png');
    return new File([blob], fileName, { type: mimeType });
}

async function handleNativeSharePayload(payload, { importFromExternalUrl, processDroppedFiles }) {
    const kind = String(payload?.kind || '').trim().toLowerCase();

    if (kind === 'url') {
        const url = normalizeHttpUrl(payload?.url);
        if (!url) {
            return;
        }

        await importFromExternalUrl(url);
        return;
    }

    if (kind === 'png') {
        const file = await fileFromSharedPath(payload);
        if (!file) {
            return;
        }

        await processDroppedFiles([file]);
    }
}

export function initializeShareTargetImport({ importFromExternalUrl, processDroppedFiles }) {
    if (!isTauriEnv) {
        return () => { /* noop */ };
    }

    const bridge = getNativeShareBridge();
    if (!bridge) {
        return () => { /* noop */ };
    }

    let processingQueue = Promise.resolve();
    const enqueue = (payload) => {
        processingQueue = processingQueue
            .then(() => handleNativeSharePayload(payload, { importFromExternalUrl, processDroppedFiles }))
            .catch((error) => {
                console.error('Failed to import shared payload:', error);
            });
    };

    const unsubscribe = bridge.subscribe(enqueue);
    return () => {
        if (typeof unsubscribe === 'function') {
            unsubscribe();
        }
    };
}
