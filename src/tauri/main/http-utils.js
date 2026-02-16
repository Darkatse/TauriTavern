export function toUrl(input) {
    try {
        if (input instanceof URL) {
            return input;
        }

        if (input instanceof Request) {
            return new URL(input.url, window.location.origin);
        }

        if (typeof input === 'string') {
            return new URL(input, window.location.origin);
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

    if (input instanceof Request) {
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
    } else if (input instanceof Request && !['GET', 'HEAD'].includes(input.method.toUpperCase())) {
        rawBody = await input.clone().text();
    }

    if (rawBody === undefined || rawBody === null) {
        return null;
    }

    if (rawBody instanceof FormData) {
        return rawBody;
    }

    if (typeof rawBody === 'string') {
        return parseMaybeJson(rawBody);
    }

    if (rawBody instanceof URLSearchParams) {
        return Object.fromEntries(rawBody.entries());
    }

    if (rawBody instanceof Blob) {
        const text = await rawBody.text();
        return parseMaybeJson(text);
    }

    if (ArrayBuffer.isView(rawBody) || rawBody instanceof ArrayBuffer) {
        const bytes = rawBody instanceof ArrayBuffer ? new Uint8Array(rawBody) : new Uint8Array(rawBody.buffer);
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
