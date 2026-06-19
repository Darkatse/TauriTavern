// @ts-check

export const CHARACTER_PATH_SEGMENT_FORBIDDEN_PATTERN = /[\/\\\u0000]/;
const CHARACTER_AVATAR_FILE_NAME_FORBIDDEN_PATTERN = /[\/\\?<>:*|"\u0000-\u001F\u0080-\u009F]/;
export const CHARACTER_AVATAR_FILE_EXTENSION = '.png';

/**
 * Upstream character APIs use avatar_url as an exact avatar file name identity.
 * It is not a URL or asset path: do not trim, decode, strip query/hash, or take basename here.
 * @param {any} value Field value
 * @param {string} fieldName Field name used in the public error
 * @param {{ required?: boolean }} [options]
 * @returns {string} Exact avatar filename, or empty string when optional and missing
 */
export function assertCharacterAvatarFileName(value, fieldName, { required = false } = {}) {
    if (value === null || value === undefined) {
        if (required) {
            throw new Error(`Bad request: no ${fieldName} in request body`);
        }
        return '';
    }

    const text = String(value);
    if (!text) {
        if (required) {
            throw new Error(`Bad request: no ${fieldName} in request body`);
        }
        return '';
    }

    if (CHARACTER_AVATAR_FILE_NAME_FORBIDDEN_PATTERN.test(text)
        || !text.endsWith(CHARACTER_AVATAR_FILE_EXTENSION)) {
        throw new Error(`Bad request: invalid ${fieldName}`);
    }

    return text;
}

/** @param {any} value */
export function hasCharacterAvatarIdentity(value) {
    return value !== null && value !== undefined && String(value).length > 0;
}

/**
 * Converts an exact avatar filename identity into the storage stem used by Rust commands.
 * @param {any} value Field value
 * @param {string} fieldName Field name used in the public error
 * @param {{ required?: boolean }} [options]
 * @returns {string} Exact storage stem, or empty string when optional and missing
 */
export function characterStemFromAvatarFileName(value, fieldName, { required = false } = {}) {
    const fileName = assertCharacterAvatarFileName(value, fieldName, { required });
    return fileName ? fileName.slice(0, -CHARACTER_AVATAR_FILE_EXTENSION.length) : '';
}
