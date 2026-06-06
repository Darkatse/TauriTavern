// @ts-check

const CHARACTER_FILE_NAME_FORBIDDEN_PATTERN = /[\/\\\u0000]/;

/**
 * Validates an upstream avatar filename identity exactly as supplied.
 * This is for APIs whose contract is an avatar file name, not a resource URL.
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

    if (CHARACTER_FILE_NAME_FORBIDDEN_PATTERN.test(text) || !text.endsWith('.png')) {
        throw new Error(`Bad request: invalid ${fieldName}`);
    }

    return text;
}

/**
 * Validates an upstream character filename field without changing caller intent.
 * @param {any} value Field value
 * @param {string} fieldName Field name used in the public error
 * @param {{ required?: boolean }} [options]
 * @returns {string} Trimmed filename, or empty string when optional and missing
 */
export function assertCharacterFileName(value, fieldName, { required = false } = {}) {
    const text = value === null || value === undefined ? '' : String(value).trim();
    if (!text) {
        if (required) {
            throw new Error(`Bad request: no ${fieldName} in request body`);
        }
        return '';
    }

    if (CHARACTER_FILE_NAME_FORBIDDEN_PATTERN.test(text)) {
        throw new Error(`Bad request: invalid ${fieldName}`);
    }

    return text;
}

/** @param {any} value */
function toRoundedInt(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
        return null;
    }

    return Math.round(number);
}

/** @param {URL} url */
export function parseCropParam(url) {
    const raw = url.searchParams.get('crop');
    if (!raw) {
        return null;
    }

    try {
        const crop = JSON.parse(raw);
        if (!crop || typeof crop !== 'object') {
            return null;
        }

        const x = toRoundedInt(crop.x);
        const y = toRoundedInt(crop.y);
        const width = toRoundedInt(crop.width);
        const height = toRoundedInt(crop.height);
        if (x === null || y === null || width === null || height === null) {
            return null;
        }

        return {
            x,
            y,
            width,
            height,
            want_resize: Boolean(crop.want_resize),
        };
    } catch {
        return null;
    }
}
