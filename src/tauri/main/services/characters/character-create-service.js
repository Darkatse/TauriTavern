// @ts-check

import { formDataToCreateCharacterDto, payloadToCreateCharacterDto } from './character-create-mapper.js';
import { parseCropParam } from './character-request-utils.js';

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>} MaterializeUploadFileFn
 */

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   materializeUploadFile: MaterializeUploadFileFn;
 * }} deps
 */
export function createCharacterCreateService({ safeInvoke, materializeUploadFile }) {
    /** @param {FormData} formData @param {URL} requestUrl */
    async function createCharacterFromForm(formData, requestUrl) {
        const dto = formDataToCreateCharacterDto(formData);
        const crop = parseCropParam(requestUrl);
        const file = formData.get('avatar');

        if (file instanceof Blob && file.size > 0) {
            const preferredName = file instanceof File ? file.name : '';
            const fileInfo = await materializeUploadFile(file, {
                preferredName,
            });
            if (!fileInfo?.filePath) {
                const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
                throw new Error(`Bad request: unable to access avatar file path${reason}`);
            }

            try {
                return await safeInvoke('create_character_with_avatar', {
                    dto: {
                        character: dto,
                        avatar_path: fileInfo.filePath,
                        crop,
                    },
                });
            } finally {
                await fileInfo.cleanup?.();
            }
        }

        return safeInvoke('create_character', { dto });
    }

    /** @param {Record<string, any>} payload */
    async function createCharacterFromPayload(payload) {
        const dto = payloadToCreateCharacterDto(payload);
        return safeInvoke('create_character', { dto });
    }

    return {
        createCharacterFromForm,
        createCharacterFromPayload,
    };
}
