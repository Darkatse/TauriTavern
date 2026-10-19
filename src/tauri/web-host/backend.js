// @ts-check

/** @param {import('./install.js').WebHostOptions['backend']} backend */
export function createBackend(backend) {
    /** @param {string} url @param {any} [body] @param {string} [method] */
    function json(url, body = {}, method = 'POST') {
        const response = backend.handle({ method, url, headers: { 'content-type': 'application/json' }, body: method === 'GET' ? null : JSON.stringify(body) });
        if (response.status < 200 || response.status >= 300) {
            throw new Error(`${method} ${url}: ${response.status} ${response.body}`);
        }
        return JSON.parse(response.body);
    }
    return { json };
}
