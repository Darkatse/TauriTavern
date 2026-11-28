// @ts-check

import { translateSillyTavern } from '../../tauri/main/adapters/st/sillytavern-i18n.js';
import { asUpstreamFailureDetails, upstreamFailureMessage } from '../../tauri/main/kernel/upstream-failure.js';

/**
 * A host error is written as `<prefix>: <message>`, and the prefix carries its kind. A
 * `CommandError` variant is written with the first prefix of its kind; the other prefixes begin
 * the messages of the Rust errors that variants wrap.
 */
const KINDS = Object.freeze([
    { variant: 'BadRequest', status: 400, prefixes: ['Bad request', 'Validation error'] },
    { variant: 'Unauthorized', status: 401, prefixes: ['Unauthorized'] },
    { variant: 'Forbidden', status: 403, prefixes: ['Forbidden', 'Permission denied'] },
    { variant: 'NotFound', status: 404, prefixes: ['Not found', 'Entity not found'] },
    { variant: 'Conflict', status: 409, prefixes: ['Conflict'] },
    { variant: 'TooManyRequests', status: 429, prefixes: ['Too many requests'] },
    { variant: 'InternalServerError', status: 500, prefixes: ['Internal server error', 'Internal error'] },
]);

/**
 * @param {unknown} value
 * @returns {string}
 */
export function extractErrorText(value) {
    if (!value) {
        return '';
    }

    if (typeof value === 'string') {
        return value.trim();
    }

    if (value instanceof Error) {
        const message = typeof value.message === 'string' ? value.message.trim() : '';
        return message || String(value);
    }

    if (typeof value === 'object') {
        const written = commandErrorText(value);
        if (written) {
            return written;
        }

        const message = /** @type {{ message?: unknown }} */ (value).message;
        if (typeof message === 'string') {
            return message.trim();
        }
    }

    try {
        return JSON.stringify(value) ?? String(value);
    } catch {
        return String(value);
    }
}

/**
 * Tauri serializes a `CommandError` as its single variant, e.g. `{ NotFound: "…" }`.
 * @param {object} value
 */
function commandErrorText(value) {
    const [entry, ...rest] = Object.entries(value);
    if (Array.isArray(value) || !entry || rest.length > 0) {
        return '';
    }

    const [variant, payload] = entry;
    if (variant === 'UpstreamFailure') {
        const details = asUpstreamFailureDetails(payload);
        if (!details) {
            return '';
        }
        const message = upstreamFailureMessage(details, translateSillyTavern);
        return details.endpoint ? `${message} (${details.endpoint})` : message;
    }

    if (typeof payload !== 'string') {
        return '';
    }
    const kind = KINDS.find(candidate => candidate.variant === variant);
    return kind ? `${kind.prefixes[0]}: ${payload.trim()}` : payload.trim();
}

/** @param {string} text */
function leadingPrefix(text) {
    const lower = text.toLowerCase();
    for (const kind of KINDS) {
        for (const prefix of kind.prefixes) {
            if (lower.startsWith(`${prefix.toLowerCase()}:`)) {
                return { kind, length: prefix.length + 1 };
            }
        }
    }
    return null;
}

/**
 * @param {unknown} value
 * @returns {number}
 */
export function commandErrorStatus(value) {
    return leadingPrefix(extractErrorText(value))?.kind.status ?? 500;
}

/**
 * The `CommandError` for an HTTP status; a status with no kind of its own is an internal error.
 * @param {unknown} status
 * @param {string} message
 * @returns {Record<string, string>}
 */
export function commandErrorForStatus(status, message) {
    const kind = KINDS.find(candidate => candidate.status === Number(status));
    return { [kind?.variant ?? 'InternalServerError']: message };
}

/**
 * @param {unknown} value
 * @returns {{ status: number, body: string }}
 */
export function commandErrorResponse(value) {
    const body = extractErrorText(value);
    return body ? { status: commandErrorStatus(body), body } : { status: 500, body: 'Internal Server Error' };
}

/**
 * Cancellation is not written into the text, so it is read from the rejection value or from the
 * `cause` an `Error` keeps.
 * @param {unknown} value
 */
export function isCommandCancelled(value) {
    const raw = value instanceof Error ? value.cause : value;
    return typeof raw === 'object' && raw !== null && Object.hasOwn(raw, 'Cancelled');
}

/** @param {unknown} value */
export function getCommandErrorMessage(value) {
    return stripCommandErrorPrefixes(extractErrorText(value));
}

/** @param {unknown} message */
export function stripCommandErrorPrefixes(message) {
    let text = String(message || '').trim();
    for (let prefix = leadingPrefix(text); prefix; prefix = leadingPrefix(text)) {
        text = text.slice(prefix.length).trim();
    }
    return text;
}
