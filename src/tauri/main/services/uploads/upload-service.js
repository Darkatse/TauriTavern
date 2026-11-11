// @ts-check

import { isMobileHost } from '../../../../scripts/util/host-identity.js';
import { createFileStagingService } from '../files/file-staging-service.js';

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any, options?: { headers?: HeadersInit }) => Promise<any>} RawInvokeFn
 */

/**
 * @param {{ safeInvoke?: SafeInvokeFn; invoke?: RawInvokeFn }} [deps]
 */
export function createUploadService({ safeInvoke, invoke } = {}) {
    const staging = createFileStagingService({ safeInvoke, invoke });

    /** @param {any} file */
    function extractNativeFilePath(file) {
        if (!file || typeof file !== 'object') {
            return null;
        }

        // @ts-ignore - non-standard fields provided by WebView file inputs.
        const candidate = file.path || file.webkitRelativePath || null;
        if (!candidate || typeof candidate !== 'string') {
            return null;
        }

        if (candidate.toLowerCase().includes('fakepath')) {
            return null;
        }

        return candidate;
    }

    /** @param {any} value */
    function isLikelyFileSystemPath(value) {
        if (typeof value !== 'string' || !value.trim()) {
            return false;
        }

        const normalized = value.trim();
        if (/^[a-z]+:\/\//i.test(normalized)) {
            return false;
        }

        return (
            normalized.startsWith('/') ||
            normalized.startsWith('\\\\') ||
            /^[a-z]:[\\/]/i.test(normalized)
        );
    }

    /** @param {string | null} filePath */
    function shouldUseDirectUploadPath(filePath) {
        if (!isLikelyFileSystemPath(filePath)) {
            return false;
        }

        // Mobile file pickers often expose scoped paths that are not directly readable by Rust.
        // Materializing into app storage keeps behavior consistent and permission-safe.
        if (isMobileHost()) {
            return false;
        }

        return true;
    }

    /**
     * @param {Blob} file
     * @param {{ preferredName?: string; preferredExtension?: string; kind?: string } | undefined} options
     * @returns {Promise<MaterializedFileInfo | null>}
     */
    async function materializeUploadFile(file, { preferredName = '', preferredExtension = '', kind = 'generic' } = {}) {
        if (!(file instanceof Blob)) {
            return null;
        }

        const directPath = extractNativeFilePath(file);
        if (shouldUseDirectUploadPath(directPath)) {
            return {
                filePath: /** @type {string} */ (directPath),
                isTemporary: false,
            };
        }

        try {
            const filePath = await staging.stageBlob(file, {
                kind,
                preferredName,
                preferredExtension,
            });
            return { filePath, isTemporary: true, cleanup: () => staging.discardFile(filePath) };
        } catch (error) {
            console.warn('Tauri host upload staging failed:', error);
            return {
                filePath: '',
                // @ts-ignore - normalize unknown error shape.
                error: error?.message || 'Failed to stage upload file',
                isTemporary: false,
            };
        }
    }

    return {
        materializeUploadFile,
    };
}
