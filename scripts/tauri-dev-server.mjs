import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendRoot = path.resolve(__dirname, '..', 'src');
const port = 1430;
const reloadPath = '/__tauritavern_dev_reload';
const reloadHost = process.env.TAURI_DEV_HOST || 'localhost';
const reloadUrl = `http://${formatUrlHost(reloadHost)}:${port}${reloadPath}`;

const reloadClientScript = `
<script type="module">
{
    const source = new EventSource(${JSON.stringify(reloadUrl)});
    source.addEventListener('reload', () => globalThis.location.reload());
}
</script>
`;

const mimeTypes = new Map([
    ['.css', 'text/css; charset=utf-8'],
    ['.gif', 'image/gif'],
    ['.html', 'text/html; charset=utf-8'],
    ['.ico', 'image/x-icon'],
    ['.jpg', 'image/jpeg'],
    ['.jpeg', 'image/jpeg'],
    ['.js', 'text/javascript; charset=utf-8'],
    ['.json', 'application/json; charset=utf-8'],
    ['.map', 'application/json; charset=utf-8'],
    ['.mjs', 'text/javascript; charset=utf-8'],
    ['.mp3', 'audio/mpeg'],
    ['.mp4', 'video/mp4'],
    ['.ogg', 'audio/ogg'],
    ['.png', 'image/png'],
    ['.svg', 'image/svg+xml'],
    ['.ttf', 'font/ttf'],
    ['.txt', 'text/plain; charset=utf-8'],
    ['.wasm', 'application/wasm'],
    ['.wav', 'audio/wav'],
    ['.webm', 'video/webm'],
    ['.webp', 'image/webp'],
    ['.woff', 'font/woff'],
    ['.woff2', 'font/woff2'],
    ['.xml', 'application/xml; charset=utf-8'],
]);

const reloadClients = new Set();
let reloadTimer;
let watcherRefreshTimer;

function formatUrlHost(host) {
    return host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
}

function isInsideFrontendRoot(filePath) {
    const relative = path.relative(frontendRoot, filePath);
    return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function contentType(filePath) {
    return mimeTypes.get(path.extname(filePath).toLowerCase()) ?? 'application/octet-stream';
}

function isReloadEntryDocument(filePath) {
    const relativePath = path.relative(frontendRoot, filePath).split(path.sep).join('/');
    return relativePath === 'index.html' || relativePath === 'login.html';
}

async function resolveStaticFile(requestUrl) {
    let pathname;
    try {
        pathname = decodeURIComponent(new URL(requestUrl, `http://localhost:${port}`).pathname);
    } catch {
        return { status: 400, message: 'Bad Request' };
    }

    if (pathname === '/') {
        pathname = '/index.html';
    }

    let filePath = path.resolve(frontendRoot, `.${pathname}`);
    if (!isInsideFrontendRoot(filePath)) {
        return { status: 403, message: 'Forbidden' };
    }

    let stat;
    try {
        stat = await fs.promises.stat(filePath);
    } catch {
        return { status: 404, message: 'Not Found' };
    }

    if (stat.isDirectory()) {
        filePath = path.join(filePath, 'index.html');
        if (!isInsideFrontendRoot(filePath)) {
            return { status: 403, message: 'Forbidden' };
        }

        try {
            stat = await fs.promises.stat(filePath);
        } catch {
            return { status: 404, message: 'Not Found' };
        }
    }

    if (!stat.isFile()) {
        return { status: 404, message: 'Not Found' };
    }

    return { filePath };
}

function injectReloadClient(filePath, data) {
    if (!isReloadEntryDocument(filePath)) {
        return data;
    }

    const html = data.toString('utf8');
    const bodyIndex = html.toLowerCase().lastIndexOf('</body>');
    if (bodyIndex === -1) {
        return Buffer.from(`${html}${reloadClientScript}`);
    }

    return Buffer.from(`${html.slice(0, bodyIndex)}${reloadClientScript}${html.slice(bodyIndex)}`);
}

function writeText(response, status, body) {
    response.writeHead(status, {
        'Cache-Control': 'no-store',
        'Content-Length': Buffer.byteLength(body),
        'Content-Type': 'text/plain; charset=utf-8',
    });
    response.end(body);
}

async function serveStatic(request, response) {
    const resolved = await resolveStaticFile(request.url ?? '/');
    if (!resolved.filePath) {
        writeText(response, resolved.status, resolved.message);
        return;
    }

    const raw = await fs.promises.readFile(resolved.filePath);
    const body = injectReloadClient(resolved.filePath, raw);
    response.writeHead(200, {
        'Cache-Control': 'no-store',
        'Content-Length': body.byteLength,
        'Content-Type': contentType(resolved.filePath),
    });

    if (request.method === 'HEAD') {
        response.end();
    } else {
        response.end(body);
    }
}

function serveReloadStream(request, response) {
    if (request.method !== 'GET') {
        writeText(response, 405, 'Method Not Allowed');
        return;
    }

    response.writeHead(200, {
        'Access-Control-Allow-Origin': '*',
        'Cache-Control': 'no-store',
        'Connection': 'keep-alive',
        'Content-Type': 'text/event-stream',
    });
    response.write('retry: 500\n\n');
    reloadClients.add(response);

    const keepAlive = setInterval(() => {
        response.write(': keep-alive\n\n');
    }, 30000);

    request.on('close', () => {
        clearInterval(keepAlive);
        reloadClients.delete(response);
    });
}

function scheduleReload() {
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(() => {
        const payload = `event: reload\ndata: ${Date.now()}\n\n`;
        for (const client of reloadClients) {
            client.write(payload);
        }
    }, 80);
}

function watchTree(root) {
    const watchers = new Map();

    const watchDirectory = (directory) => {
        if (watchers.has(directory)) {
            return;
        }

        const watcher = fs.watch(directory, (eventType) => {
            scheduleReload();
            if (eventType === 'rename') {
                scheduleWatcherRefresh();
            }
        });
        watchers.set(directory, watcher);
    };

    const scan = (directory) => {
        watchDirectory(directory);
        for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
            if (entry.isDirectory()) {
                scan(path.join(directory, entry.name));
            }
        }
    };

    const scheduleWatcherRefresh = () => {
        clearTimeout(watcherRefreshTimer);
        watcherRefreshTimer = setTimeout(() => scan(root), 200);
    };

    scan(root);

    return () => {
        for (const watcher of watchers.values()) {
            watcher.close();
        }
        watchers.clear();
    };
}

function watchFrontend() {
    try {
        const watcher = fs.watch(frontendRoot, { recursive: true }, scheduleReload);
        return () => watcher.close();
    } catch {
        return watchTree(frontendRoot);
    }
}

const closeWatcher = watchFrontend();
const server = http.createServer((request, response) => {
    const pathname = new URL(request.url ?? '/', `http://localhost:${port}`).pathname;
    if (pathname === reloadPath) {
        serveReloadStream(request, response);
        return;
    }

    if (request.method !== 'GET' && request.method !== 'HEAD') {
        writeText(response, 405, 'Method Not Allowed');
        return;
    }

    serveStatic(request, response).catch((error) => {
        console.error(error);
        writeText(response, 500, 'Internal Server Error');
    });
});

server.on('error', (error) => {
    closeWatcher();
    console.error(error);
    process.exit(1);
});

function shutdown() {
    closeWatcher();
    clearTimeout(reloadTimer);
    clearTimeout(watcherRefreshTimer);
    for (const client of reloadClients) {
        client.end();
    }
    reloadClients.clear();
    server.close(() => process.exit(0));
}

process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);

server.listen(port, () => {
    console.log(`TauriTavern frontend dev server listening on http://localhost:${port}`);
    console.log(`TauriTavern reload endpoint advertised as ${reloadUrl}`);
});
