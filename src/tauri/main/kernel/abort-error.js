export function createAbortError(message = 'The operation was aborted.') {
    const text = typeof message === 'string' && message.trim() ? message : 'The operation was aborted.';

    if (typeof DOMException === 'function') {
        return new DOMException(text, 'AbortError');
    }

    const error = new Error(text);
    error.name = 'AbortError';
    return error;
}

export function isAbortError(error) {
    if (!error || typeof error !== 'object') {
        return false;
    }

    return error.name === 'AbortError';
}
