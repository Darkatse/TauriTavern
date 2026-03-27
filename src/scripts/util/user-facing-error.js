import { translate } from '../i18n.js';
import { stripCommandErrorPrefixes } from './command-error-utils.js';

export function extractErrorText(value) {
    if (!value) {
        return '';
    }

    if (typeof value === 'string') {
        return value.trim();
    }

    if (value instanceof Error) {
        const message = typeof value.message === 'string' ? value.message.trim() : '';
        return message || String(value).trim();
    }

    if (typeof value?.message === 'string') {
        return value.message.trim();
    }

    return String(value).trim();
}

export function toUserFacingErrorText(value) {
    const normalized = stripCommandErrorPrefixes(extractErrorText(value));
    return normalized ? translate(normalized) : '';
}

