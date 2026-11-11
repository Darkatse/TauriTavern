// @ts-check

/** @param {import('./install.js').WebHostOptions['backend']} backend */
export function createBackend(backend) {
    /** @param {string} url @param {import('./install.js').BackendRequest['body']} body @param {string} [method] */
    function send(url, body, method = 'POST') {
        const headers = typeof body === 'string' ? { 'content-type': 'application/json' } : {};
        return backend.handle({ method, url, headers, body });
    }
    /** @param {string} url @param {import('./install.js').BackendRequest['body']} body @param {string} [method] */
    function request(url, body, method = 'POST') {
        const response = send(url, body, method);
        if (response.status < 200 || response.status >= 300) {
            throw new Error(`${method} ${url}: ${response.status} ${response.body}`);
        }
        return response;
    }
    /** @param {string} url @param {any} [body] @param {string} [method] */
    function json(url, body = {}, method = 'POST') {
        const response = request(url, method === 'GET' ? null : JSON.stringify(body), method);
        if (typeof response.body !== 'string') throw new Error(`${method} ${url}: expected a JSON response`);
        return JSON.parse(response.body);
    }
    return { json, request, send };
}
