// The native share bridge delivers payloads it has already normalized.
async function handleNativeSharePayload(payload, { importFromExternalUrl, processDroppedFiles }) {
    switch (payload.kind) {
        case 'url':
            await importFromExternalUrl(payload.url);
            return;
        case 'png':
            await processDroppedFiles([
                await window.__TAURITAVERN__.api.characterCards.stageSharedFile(payload.uri, payload.name),
            ]);
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
