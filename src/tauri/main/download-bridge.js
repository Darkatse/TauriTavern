export function createDownloadBridge({
    isNativeMobileDownloadRuntime,
    downloadBlobWithRuntime,
    notifyDownloadResult,
    notifyDownloadError,
    fallbackName = 'download.bin',
}) {
    const patchStateByWindow = new WeakMap();
    const trackedBlobUrls = new Map();

    function isBlobLike(value) {
        return !!value
            && typeof value === 'object'
            && typeof value.arrayBuffer === 'function'
            && typeof value.type === 'string';
    }

    function getPatchState(targetWindow) {
        const state = patchStateByWindow.get(targetWindow);
        if (state) {
            return state;
        }

        const nextState = {
            currentDocument: null,
            documentListener: null,
            patchedCreateObjectURL: null,
            patchedRevokeObjectURL: null,
            patchedAnchorClick: null,
        };
        patchStateByWindow.set(targetWindow, nextState);
        return nextState;
    }

    function getDownloadFileName(anchorElement) {
        const attributeValue = anchorElement.getAttribute('download');
        const rawName = attributeValue ?? anchorElement.download ?? '';
        return String(rawName || '').trim() || fallbackName;
    }

    function resolveDownloadAnchor(targetWindow, eventTarget) {
        if (!(eventTarget instanceof targetWindow.Node)) {
            return null;
        }

        if (eventTarget instanceof targetWindow.HTMLAnchorElement) {
            return eventTarget;
        }

        if (!(eventTarget instanceof targetWindow.Element)) {
            return null;
        }

        const anchorElement = eventTarget.closest('a');
        return anchorElement instanceof targetWindow.HTMLAnchorElement ? anchorElement : null;
    }

    async function readHrefAsBlob(targetWindow, href) {
        const response = await targetWindow.fetch(href);
        if (!response?.ok) {
            throw new Error(`Failed to read download payload: ${response?.status || 'unknown error'}`);
        }

        return response.blob();
    }

    function resolveSameOriginDownloadUrl(targetWindow, href) {
        try {
            const url = new targetWindow.URL(href, targetWindow.location.href);
            if (url.origin !== targetWindow.location.origin) {
                return null;
            }

            if (url.protocol === 'blob:' || url.protocol === 'data:' || url.protocol === 'javascript:') {
                return null;
            }

            return url.href;
        } catch {
            return null;
        }
    }

    function createDownloadRequest(targetWindow, anchorElement) {
        if (!anchorElement.hasAttribute('download')) {
            return null;
        }

        const href = String(anchorElement.href || '').trim();
        if (!href) {
            return null;
        }

        const fileName = getDownloadFileName(anchorElement);

        if (href.startsWith('blob:')) {
            const blob = trackedBlobUrls.get(href);
            return {
                fileName,
                blobPromise: blob ? Promise.resolve(blob) : readHrefAsBlob(targetWindow, href),
            };
        }

        if (href.startsWith('data:')) {
            return {
                fileName,
                blobPromise: readHrefAsBlob(targetWindow, href),
            };
        }

        const sameOriginUrl = resolveSameOriginDownloadUrl(targetWindow, href);
        if (!sameOriginUrl) {
            return null;
        }

        return {
            fileName,
            blobPromise: readHrefAsBlob(targetWindow, sameOriginUrl),
        };
    }

    async function handleDownloadRequest(request) {
        const result = await downloadBlobWithRuntime(await request.blobPromise, request.fileName, {
            fallbackName,
        });

        if (typeof notifyDownloadResult !== 'function') {
            return;
        }

        try {
            notifyDownloadResult(result);
        } catch (error) {
            console.warn('Failed to show download feedback:', error);
        }
    }

    function notifyDownloadFailure(error) {
        if (typeof notifyDownloadError !== 'function') {
            return;
        }

        try {
            notifyDownloadError(error);
        } catch (feedbackError) {
            console.warn('Failed to show download failure feedback:', feedbackError);
        }
    }

    function bridgeAnchorDownload(targetWindow, anchorElement, event = null) {
        const request = createDownloadRequest(targetWindow, anchorElement);
        if (!request) {
            return false;
        }

        event?.preventDefault();
        void handleDownloadRequest(request).catch((error) => {
            console.error('Failed to bridge native mobile download:', error);
            notifyDownloadFailure(error);
        });
        return true;
    }

    function patchWindow(targetWindow = window) {
        if (!targetWindow || !isNativeMobileDownloadRuntime()) {
            return;
        }

        let urlApi;
        let targetDocument;
        try {
            urlApi = targetWindow.URL;
            targetDocument = targetWindow.document;
        } catch {
            return;
        }

        if (!urlApi || !targetDocument) {
            return;
        }

        const state = getPatchState(targetWindow);
        const currentCreateObjectURL = urlApi.createObjectURL;
        const currentRevokeObjectURL = urlApi.revokeObjectURL;
        const anchorPrototype = targetWindow.HTMLAnchorElement?.prototype;
        const currentAnchorClick = anchorPrototype?.click;

        if (typeof currentCreateObjectURL === 'function' && state.patchedCreateObjectURL !== currentCreateObjectURL) {
            const delegateCreateObjectURL = currentCreateObjectURL.bind(urlApi);
            const patchedCreateObjectURL = function patchedCreateObjectURL(value) {
                const objectUrl = delegateCreateObjectURL(value);
                if (typeof objectUrl === 'string' && objectUrl.startsWith('blob:') && isBlobLike(value)) {
                    trackedBlobUrls.set(objectUrl, value);
                }
                return objectUrl;
            };

            try {
                urlApi.createObjectURL = patchedCreateObjectURL;
                state.patchedCreateObjectURL = patchedCreateObjectURL;
            } catch {
                // Ignore non-writable URL bindings.
            }
        }

        if (typeof currentRevokeObjectURL === 'function' && state.patchedRevokeObjectURL !== currentRevokeObjectURL) {
            const delegateRevokeObjectURL = currentRevokeObjectURL.bind(urlApi);
            const patchedRevokeObjectURL = function patchedRevokeObjectURL(objectUrl) {
                trackedBlobUrls.delete(String(objectUrl || ''));
                return delegateRevokeObjectURL(objectUrl);
            };

            try {
                urlApi.revokeObjectURL = patchedRevokeObjectURL;
                state.patchedRevokeObjectURL = patchedRevokeObjectURL;
            } catch {
                // Ignore non-writable URL bindings.
            }
        }

        if (typeof currentAnchorClick === 'function' && state.patchedAnchorClick !== currentAnchorClick) {
            const delegateAnchorClick = currentAnchorClick;
            const patchedAnchorClick = function patchedAnchorClick(...args) {
                if (this instanceof targetWindow.HTMLAnchorElement && !this.isConnected) {
                    if (bridgeAnchorDownload(targetWindow, this)) {
                        return;
                    }
                }

                return delegateAnchorClick.apply(this, args);
            };

            try {
                anchorPrototype.click = patchedAnchorClick;
                state.patchedAnchorClick = patchedAnchorClick;
            } catch {
                // Ignore non-writable prototype bindings.
            }
        }

        if (state.currentDocument === targetDocument && typeof state.documentListener === 'function') {
            return;
        }

        if (state.currentDocument && typeof state.documentListener === 'function') {
            try {
                state.currentDocument.removeEventListener('click', state.documentListener, true);
            } catch {
                // Ignore detached documents.
            }
        }

        const documentListener = (event) => {
            const anchorElement = resolveDownloadAnchor(targetWindow, event.target);
            if (!anchorElement) {
                return;
            }

            bridgeAnchorDownload(targetWindow, anchorElement, event);
        };

        targetDocument.addEventListener('click', documentListener, true);
        state.currentDocument = targetDocument;
        state.documentListener = documentListener;
    }

    return {
        patchWindow,
    };
}
