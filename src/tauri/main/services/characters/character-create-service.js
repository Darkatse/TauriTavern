// @ts-check

import { formDataToCreateCharacterDto, payloadToCreateCharacterDto } from './character-create-mapper.js';
import { parseCropParam } from './character-request-utils.js';

export const CHARACTER_CREATE_WARNINGS = Object.freeze({
    AVATAR_IMPORT_FAILED: 'avatar-import-failed',
});

/**
 * @typedef {import('../../context/types.js').MaterializedFileInfo} MaterializedFileInfo
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(file: Blob, options?: { preferredName?: string; preferredExtension?: string }) => Promise<MaterializedFileInfo | null>} MaterializeUploadFileFn
 * @typedef {{ code: string; message: string }} CharacterCreateWarning
 * @typedef {{ character: any; warnings: CharacterCreateWarning[] }} CharacterCreateOutcome
 */

/** @param {any} character @param {CharacterCreateWarning[]} [warnings] @returns {CharacterCreateOutcome} */
function createOutcome(character, warnings = []) {
    return { character, warnings };
}

/** @param {any} result @returns {CharacterCreateOutcome} */
function createOutcomeFromCommandResult(result) {
    if (result && typeof result === 'object' && 'character' in result && Array.isArray(result.warnings)) {
        return createOutcome(result.character, result.warnings);
    }

    return createOutcome(result);
}

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
                const message = `Unable to access avatar file path${reason}`;
                console.warn('TauriTavern character create avatar import failed; using default avatar.', {
                    message,
                });
                const character = await safeInvoke('create_character', { dto });
                return createOutcome(character, [{
                    code: CHARACTER_CREATE_WARNINGS.AVATAR_IMPORT_FAILED,
                    message,
                }]);
            }

            try {
                const result = await safeInvoke('create_character_with_avatar', {
                    dto: {
                        character: dto,
                        avatar_path: fileInfo.filePath,
                        crop,
                    },
                });
                return createOutcomeFromCommandResult(result);
            } finally {
                await fileInfo.cleanup?.();
            }
        }

        const character = await safeInvoke('create_character', { dto });
        return createOutcome(character);
    }

    /** @param {Record<string, any>} payload */
    async function createCharacterFromPayload(payload) {
        const dto = payloadToCreateCharacterDto(payload);
        const character = await safeInvoke('create_character', { dto });
        return createOutcome(character);
    }

    return {
        createCharacterFromForm,
        createCharacterFromPayload,
    };
}
