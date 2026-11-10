// @ts-check

import { extractErrorText } from '../../kernel/host-error-response.js';
import { createFileStagingService } from '../files/file-staging-service.js';

/**
 * @typedef {import('../../context/types.js').MaterializeUploadFileFn} MaterializeUploadFileFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any, options?: { headers?: HeadersInit }) => Promise<any>} RawInvokeFn
 */

/**
 * @param {{ safeInvoke?: SafeInvokeFn; invoke?: RawInvokeFn }} [deps]
 */
export function createUploadService({ safeInvoke, invoke } = {}) {
    const staging = createFileStagingService({ safeInvoke, invoke });

    /**
     * Copies page bytes into host staging so Rust can read them by path.
     * The caller owns the staged copy and must call `cleanup` when done.
     * @type {MaterializeUploadFileFn}
     */
    async function materializeUploadFile(file, { preferredName = '', preferredExtension = '', kind = 'generic' } = {}) {
        let filePath;
        try {
            filePath = await staging.stageBlob(file, { kind, preferredName, preferredExtension });
        } catch (error) {
            throw new Error(`Failed to stage upload: ${extractErrorText(error)}`, { cause: error });
        }
        return { filePath, cleanup: () => staging.discardFile(filePath) };
    }

    return {
        materializeUploadFile,
    };
}
