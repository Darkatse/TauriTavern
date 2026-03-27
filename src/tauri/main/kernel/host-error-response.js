import { GITHUB_RATE_LIMIT_MESSAGE } from '../../../scripts/util/github-rate-limit.js';

export function extractErrorText(error) {
    if (!error) {
        return '';
    }

    if (typeof error === 'string') {
        return error.trim();
    }

    if (error instanceof Error) {
        const message = typeof error.message === 'string' ? error.message.trim() : '';
        return message || String(error).trim();
    }

    if (typeof error?.message === 'string') {
        return error.message.trim();
    }

    try {
        const serialized = JSON.stringify(error);
        return serialized && serialized !== '{}' ? serialized : String(error).trim();
    } catch {
        return String(error).trim();
    }
}

export function resolveHostErrorResponse(message) {
    const normalized = String(message || '').trim();
    if (!normalized) {
        return { status: 500, body: 'Internal Server Error' };
    }

    if (normalized === GITHUB_RATE_LIMIT_MESSAGE) {
        return { status: 429, body: GITHUB_RATE_LIMIT_MESSAGE };
    }

    const lower = normalized.toLowerCase();
    if (lower.startsWith('bad request:') || lower.startsWith('validation error:')) {
        return { status: 400, body: normalized };
    }
    if (lower.startsWith('unauthorized:') || lower.startsWith('permission denied:')) {
        return { status: 401, body: normalized };
    }
    if (lower.startsWith('not found:') || lower.startsWith('entity not found:')) {
        return { status: 404, body: normalized };
    }

    return { status: 500, body: normalized };
}

