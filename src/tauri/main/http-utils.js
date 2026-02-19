function getTypeTag(value) {
    return Object.prototype.toString.call(value);
}

function isRequestLike(value) {
    if (!value || typeof value !== 'object') {
        return false;
    }

    return typeof value.url === 'string'
        && typeof value.method === 'string'
        && typeof value.clone === 'function';
}

function isFormDataLike(value) {
    return getTypeTag(value) === '[object FormData]';
}

function isUrlSearchParamsLike(value) {
    return getTypeTag(value) === '[object URLSearchParams]';
}

function isBlobLike(value) {
    return getTypeTag(value) === '[object Blob]';
}

function isArrayBufferLike(value) {
    return getTypeTag(value) === '[object ArrayBuffer]';
}

function resolveBaseUrl(baseUrl) {
    return String(baseUrl || window.location.origin);
}

export function toUrl(input, baseUrl = window.location.origin) {
    const resolvedBaseUrl = resolveBaseUrl(baseUrl);
    try {
        if (input instanceof URL) {
            return input;
        }

        if (isRequestLike(input)) {
            return new URL(input.url, resolvedBaseUrl);
        }

        if (typeof input === 'string') {
            return new URL(input, resolvedBaseUrl);
        }
    } catch {
        return null;
    }

    return null;
}

export function getMethodHint(input, init) {
    if (init?.method) {
        return String(init.method).toUpperCase();
    }

    if (isRequestLike(input)) {
        return String(input.method || 'GET').toUpperCase();
    }

    return 'GET';
}

export async function getMethod(input, init) {
    return getMethodHint(input, init);
}

export async function readRequestBody(input, init) {
    let rawBody;

    if (init && Object.prototype.hasOwnProperty.call(init, 'body')) {
        rawBody = init.body;
    } else if (isRequestLike(input) && !['GET', 'HEAD'].includes(String(input.method).toUpperCase())) {
        rawBody = await input.clone().text();
    }

    if (rawBody === undefined || rawBody === null) {
        return null;
    }

    if (isFormDataLike(rawBody)) {
        return rawBody;
    }

    if (typeof rawBody === 'string') {
        return parseMaybeJson(rawBody);
    }

    if (isUrlSearchParamsLike(rawBody)) {
        return Object.fromEntries(rawBody.entries());
    }

    if (isBlobLike(rawBody)) {
        const text = await rawBody.text();
        return parseMaybeJson(text);
    }

    if (ArrayBuffer.isView(rawBody) || isArrayBufferLike(rawBody)) {
        const bytes = isArrayBufferLike(rawBody) ? new Uint8Array(rawBody) : new Uint8Array(rawBody.buffer);
        const text = new TextDecoder().decode(bytes);
        return parseMaybeJson(text);
    }

    return rawBody;
}

export function parseMaybeJson(value) {
    const text = String(value || '').trim();
    if (!text) {
        return {};
    }

    try {
        return JSON.parse(text);
    } catch {
        return text;
    }
}

export async function safeJson(response) {
    try {
        return await response.json();
    } catch {
        return {};
    }
}

export function jsonResponse(data, status = 200) {
    return new Response(JSON.stringify(data), {
        status,
        headers: {
            'Content-Type': 'application/json',
        },
    });
}

export function textResponse(text, status = 200) {
    return new Response(String(text), {
        status,
        headers: {
            'Content-Type': 'text/plain; charset=utf-8',
        },
    });
}
