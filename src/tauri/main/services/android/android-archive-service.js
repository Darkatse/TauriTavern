// @ts-check

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 * @typedef {import('../../context/types.js').AndroidExportResult} AndroidExportResult
 */

/**
 * @typedef {{
 *   stageContentUriToFile: (contentUri: string, targetFilePath: string) => string;
 *   requestImportArchivePicker: () => void;
 *   requestExportArchivePicker: (preferredName: string) => void;
 *   copyFileToContentUri: (sourcePath: string, contentUri: string) => string;
 * }} AndroidImportArchiveBridge
 */

/**
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 */

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   removeTempUploadFile: (filePath: string, invokeApi: Function) => Promise<void>;
 *   bridgeName: string;
 * }} deps
 */
export function createAndroidArchiveService({ safeInvoke, removeTempUploadFile, bridgeName }) {
    /** @type {{ resolve: (value: string) => void; reject: (reason: unknown) => void } | null} */
    let androidImportArchivePickerPending = null;
    /** @type {{ resolve: (value: string) => void; reject: (reason: unknown) => void } | null} */
    let androidExportArchivePickerPending = null;

    /** @returns {AndroidImportArchiveBridge | null} */
    function getAndroidImportArchiveBridge() {
        if (typeof window === 'undefined') {
            return null;
        }

        // @ts-ignore - dynamic bridge injected by Android host.
        return window[bridgeName] || null;
    }

    /**
     * @param {'__TAURITAVERN_IMPORT_ARCHIVE_PICKER__' | '__TAURITAVERN_EXPORT_ARCHIVE_PICKER__'} receiverName
     * @param {{
     *   getPending: () => ({ resolve: (value: string) => void; reject: (reason: unknown) => void } | null);
     *   clearPending: () => void;
     *   missingContentUriMessage: string;
     * }} options
     */
    function ensureAndroidContentUriPickerReceiver(receiverName, {
        getPending,
        clearPending,
        missingContentUriMessage,
    }) {
        if (typeof window === 'undefined') {
            return;
        }

        if (window[receiverName]?.onNativeResult) {
            return;
        }

        window[receiverName] = {
            /** @param {any} payload */
            onNativeResult(payload) {
                const pending = getPending();
                clearPending();
                if (!pending) {
                    return;
                }

                const error = String(payload?.error || '').trim();
                if (error) {
                    pending.reject(new Error(error));
                    return;
                }

                const contentUri = String(payload?.content_uri || '').trim();
                if (!contentUri) {
                    pending.reject(new Error(missingContentUriMessage));
                    return;
                }

                pending.resolve(contentUri);
            },
        };
    }

    function ensureAndroidImportArchivePickerReceiver() {
        ensureAndroidContentUriPickerReceiver('__TAURITAVERN_IMPORT_ARCHIVE_PICKER__', {
            getPending: () => androidImportArchivePickerPending,
            clearPending: () => {
                androidImportArchivePickerPending = null;
            },
            missingContentUriMessage: 'Android import picker did not return a content URI',
        });
    }

    function ensureAndroidExportArchivePickerReceiver() {
        ensureAndroidContentUriPickerReceiver('__TAURITAVERN_EXPORT_ARCHIVE_PICKER__', {
            getPending: () => androidExportArchivePickerPending,
            clearPending: () => {
                androidExportArchivePickerPending = null;
            },
            missingContentUriMessage: 'Android export picker did not return a content URI',
        });
    }

    /**
     * @param {{ join: (...paths: string[]) => Promise<string> }} pathApi
     */
    async function resolveAndroidImportStagingDirectory(pathApi) {
        const importsRoot = String(await safeInvoke('get_data_archive_imports_root')).trim();
        return pathApi.join(importsRoot, 'incoming');
    }

    /**
     * @param {string} contentUri
     * @returns {Promise<MaterializedFileInfo>}
     */
    async function materializeAndroidContentUriUpload(contentUri) {
        const bridge = getAndroidImportArchiveBridge();
        if (!bridge) {
            throw new Error('Android archive bridge is unavailable');
        }
        const pathApi = window.__TAURI__.path;

        const stagingDirectory = await resolveAndroidImportStagingDirectory(pathApi);
        const targetFileName = `tauritavern-import-${Date.now()}-${Math.random().toString(16).slice(2)}.zip`;
        const targetFilePath = await pathApi.join(stagingDirectory, targetFileName);
        const filePath = String(
            bridge.stageContentUriToFile(String(contentUri).trim(), targetFilePath),
        ).trim();
        if (!filePath) {
            throw new Error('Android import bridge did not return a file path');
        }

        const invokeApi = window.__TAURI__?.core?.invoke;

        return {
            filePath,
            isTemporary: true,
            cleanup: typeof invokeApi === 'function'
                ? async () => {
                    try {
                        await removeTempUploadFile(filePath, invokeApi);
                    } catch {
                        // noop
                    }
                }
                : undefined,
        };
    }

    function pickAndroidImportArchive() {
        const bridge = getAndroidImportArchiveBridge();
        if (!bridge) {
            throw new Error('Android archive bridge is unavailable');
        }
        ensureAndroidImportArchivePickerReceiver();
        if (androidImportArchivePickerPending) {
            throw new Error('Android import picker is already active');
        }

        return new Promise((resolve, reject) => {
            androidImportArchivePickerPending = { resolve, reject };

            try {
                bridge.requestImportArchivePicker();
            } catch (error) {
                androidImportArchivePickerPending = null;
                reject(error);
            }
        });
    }

    function pickAndroidExportArchiveDestination(preferredName = 'tauritavern-data.zip') {
        const bridge = getAndroidImportArchiveBridge();
        if (!bridge) {
            throw new Error('Android archive bridge is unavailable');
        }
        ensureAndroidExportArchivePickerReceiver();
        if (androidExportArchivePickerPending) {
            throw new Error('Android export picker is already active');
        }

        const normalizedName = String(preferredName || 'tauritavern-data.zip').trim() || 'tauritavern-data.zip';
        return new Promise((resolve, reject) => {
            androidExportArchivePickerPending = { resolve, reject };

            try {
                bridge.requestExportArchivePicker(normalizedName);
            } catch (error) {
                androidExportArchivePickerPending = null;
                reject(error);
            }
        });
    }

    /**
     * @param {string} sourcePath
     * @param {string} preferredName
     * @returns {Promise<AndroidExportResult>}
     */
    async function saveAndroidExportArchive(sourcePath, preferredName = 'tauritavern-data.zip') {
        const bridge = getAndroidImportArchiveBridge();
        if (!bridge) {
            throw new Error('Android archive bridge is unavailable');
        }
        const archivePath = String(sourcePath || '').trim();
        if (!archivePath) {
            throw new Error('Export archive path is missing');
        }

        const contentUri = await pickAndroidExportArchiveDestination(preferredName);

        const savedTarget = String(
            bridge.copyFileToContentUri(archivePath, contentUri),
        ).trim();

        return {
            savedTarget: savedTarget || contentUri,
        };
    }

    return {
        materializeAndroidContentUriUpload,
        pickAndroidImportArchive,
        saveAndroidExportArchive,
    };
}
