// @ts-check

import { setByPath, unsetByPath } from './form-object-utils.js';
import { buildCharacterCard, formDataToCharacterPayload } from './character-create-mapper.js';
import { assertCharacterAvatarFileName } from './character-identity.js';
import { parseCropParam } from './character-request-utils.js';

/**
 * @typedef {import('../../context/types.js').MaterializeUploadFileFn} MaterializeUploadFileFn
 * @typedef {(command: import('../../context/types.js').TauriInvokeCommand, args?: any) => Promise<any>} SafeInvokeFn
 * @typedef {(options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>} ResolveCharacterIdFn
 * @typedef {(options?: { avatar?: any; fallbackName?: string }) => Promise<string | null>} ResolveExistingCharacterIdFn
 */

/**
 * @param {{
 *   safeInvoke: SafeInvokeFn;
 *   resolveCharacterId: ResolveCharacterIdFn;
 *   resolveExistingCharacterId: ResolveExistingCharacterIdFn;
 *   materializeUploadFile: MaterializeUploadFileFn;
 * }} deps
 */
export function createCharacterFormService({
    safeInvoke,
    resolveCharacterId,
    resolveExistingCharacterId,
    materializeUploadFile,
}) {
    /** @param {FormData} formData @param {string} key @param {string} [fallback] */
    function stringFromForm(formData, key, fallback = '') {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return fallback;
        }

        return String(raw);
    }

    /** @param {FormData} formData */
    function avatarUrlFromForm(formData) {
        return assertCharacterAvatarFileName(formData.get('avatar_url'), 'avatar_url', { required: true });
    }

    /** @param {FormData} formData */
    function buildCharacterCardFromForm(formData) {
        const card = buildCharacterCard(formDataToCharacterPayload(formData));
        const chat = formData.has('chat')
            ? stringFromForm(formData, 'chat', '').trim()
            : `${card.name} - ${new Date().toISOString()}`;
        const createDate = stringFromForm(formData, 'create_date', '').trim();

        if (formData.has('chat')) {
            if (chat) {
                setByPath(card, 'chat', chat);
            } else {
                unsetByPath(card, 'chat');
            }
        } else {
            setByPath(card, 'chat', chat);
        }

        if (formData.has('create_date')) {
            if (createDate) {
                setByPath(card, 'create_date', createDate);
            } else {
                unsetByPath(card, 'create_date');
            }
        }

        return card;
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function editCharacterFromForm(formData, requestUrl) {
        const avatar = avatarUrlFromForm(formData);
        const originalCharacterId = await resolveCharacterId({ avatar });

        if (!originalCharacterId) {
            throw new Error('Character not found for edit');
        }

        const file = formData.get('avatar');
        const crop = parseCropParam(requestUrl);
        const card = buildCharacterCardFromForm(formData);

        if (file instanceof Blob && file.size > 0) {
            const preferredName = file instanceof File ? file.name : '';
            const fileInfo = await materializeUploadFile(file, {
                kind: 'avatar',
                preferredName,
            });

            try {
                await safeInvoke('update_character_card_data', {
                    name: originalCharacterId,
                    dto: {
                        card_json: JSON.stringify(card),
                        avatar_path: fileInfo.filePath,
                        crop: crop || null,
                        materialize_primary_lorebook: true,
                    },
                });
            } finally {
                await fileInfo.cleanup();
            }

            return;
        }

        await safeInvoke('update_character_card_data', {
            name: originalCharacterId,
            dto: {
                card_json: JSON.stringify(card),
                avatar_path: null,
                crop: crop || null,
                materialize_primary_lorebook: true,
            },
        });
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function editCharacterAvatarFromForm(formData, requestUrl) {
        const avatar = avatarUrlFromForm(formData);
        const file = formData.get('avatar');
        if (!(file instanceof Blob) || file.size === 0) {
            throw new Error('Bad request: no file uploaded');
        }

        const characterId = await resolveExistingCharacterId({ avatar });
        if (!characterId) {
            throw new Error('Bad request: character file does not exist');
        }

        const crop = parseCropParam(requestUrl);
        const preferredName = file instanceof File ? file.name : '';
        const fileInfo = await materializeUploadFile(file, {
            kind: 'avatar',
            preferredName,
        });

        try {
            await safeInvoke('update_avatar', {
                dto: {
                    name: characterId,
                    avatar_path: fileInfo.filePath,
                    crop: crop || null,
                },
            });
        } finally {
            await fileInfo.cleanup();
        }
    }

    /** @param {FormData} formData @param {URL} requestUrl */
    async function uploadAvatarFromForm(formData, requestUrl) {
        const file = formData.get('avatar');
        if (!(file instanceof Blob)) {
            throw new Error('Bad request: no avatar file provided');
        }

        const overwriteNameRaw = formData.get('overwrite_name');
        const overwriteName = overwriteNameRaw ? String(overwriteNameRaw) : null;
        const crop = parseCropParam(requestUrl);

        const preferredName = file instanceof File ? file.name : '';
        const fileInfo = await materializeUploadFile(file, {
            kind: 'user-avatar',
            preferredName,
        });

        try {
            const uploaded = await safeInvoke('upload_avatar', {
                file_path: fileInfo.filePath,
                overwrite_name: overwriteName,
                crop: crop || null,
            });
            return uploaded;
        } finally {
            await fileInfo.cleanup();
        }
    }

    return {
        editCharacterFromForm,
        editCharacterAvatarFromForm,
        uploadAvatarFromForm,
    };
}
