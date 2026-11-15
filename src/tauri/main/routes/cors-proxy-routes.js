import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { streamResponse, textResponse, toRequest } from '../http-utils.js';
import { encodeIpcByteBody } from '../services/files/ipc-byte-body.js';
import { createReadableFileStreamService } from '../services/files/readable-file-stream-service.js';

const REQUEST_HEADER = 'cors-proxy-request';

/**
 * Upstream's `/proxy/<url>`: the host sends the page's request to `<url>` and returns the response as received.
 * Both `'/proxy/' + url` and `'/proxy/' + encodeURIComponent(url)` name the same target.
 */
export function registerCorsProxyRoutes(router, context) {
    const { createByteStream } = createReadableFileStreamService({
        invoke: (...args) => context.invokeTransport(...args),
    });

    router.all('/proxy/*', async ({ url, wildcard, input, init }) => {
        let target;
        try {
            target = decodeURIComponent(wildcard) + url.search;
        } catch {
            return textResponse(`Bad request: invalid proxy target: ${wildcard}`, 400);
        }

        const request = toRequest(url, input, init);
        const { body, headers } = encodeIpcByteBody(new Uint8Array(await request.arrayBuffer()));
        const metadata = {
            method: request.method,
            url: target,
            headers: [...request.headers],
            locale: getSillyTavernLocale(),
        };
        const response = await context.invokeTransport('send_cors_proxy_request', body, {
            headers: { ...headers, [REQUEST_HEADER]: encodeURIComponent(JSON.stringify(metadata)) },
        });

        const responseInit = {
            status: response.status,
            statusText: response.statusText,
            headers: response.headers,
        };
        return response.rid === null
            ? new Response(null, responseInit)
            : streamResponse(createByteStream(response.rid), responseInit);
    }, { body: 'none' });
}
