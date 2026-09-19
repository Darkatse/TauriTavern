import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { subscribe } from '@parcel/watcher';
import { rspack } from '@rspack/core';

import { createRspackConfigs } from '../rspack.config.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendRoot = path.resolve(__dirname, '..', 'src');
const port = 1430;
const reloadPath = '/__tauritavern_dev_reload';
const reloadHost = process.env.TAURI_DEV_HOST || 'localhost';
const reloadUrl = `http://${formatUrlHost(reloadHost)}:${port}${reloadPath}`;
const devServiceWorkerBootstrap = '<script src="/dev-sw-bootstrap.js"></script>';
const rspackConfigs = createRspackConfigs('development');
const bundleOutputRoots = rspackConfigs.map(config => config.output.path);

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
const bundledFiles = new Set();
let reloadTimer;
let closeBundleWatcher;
let closeFrontendWatcher;
let shuttingDown = false;

function formatUrlHost(host) {
    return host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
}

function isInsideFrontendRoot(filePath) {
    const relative = path.relative(frontendRoot, filePath);
    return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function isInside(filePath, directory) {
    const relative = path.relative(directory, filePath);
    return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

function isRspackOwned(filePath) {
    const resolved = path.resolve(filePath);
    return bundledFiles.has(resolved)
        || bundleOutputRoots.some(directory => isInside(resolved, directory));
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

function injectDevRuntime(filePath, data) {
    if (!isReloadEntryDocument(filePath)) {
        return data;
    }

    const html = data.toString('utf8');
    const headTag = '<head>';
    const headIndex = html.toLowerCase().indexOf(headTag);
    const bootstrapIndex = headIndex === -1 ? 0 : headIndex + headTag.length;
    const bootstrappedHtml = `${html.slice(0, bootstrapIndex)}${devServiceWorkerBootstrap}${html.slice(bootstrapIndex)}`;
    const bodyIndex = bootstrappedHtml.toLowerCase().lastIndexOf('</body>');
    if (bodyIndex === -1) {
        return Buffer.from(`${bootstrappedHtml}${reloadClientScript}`);
    }

    return Buffer.from(`${bootstrappedHtml.slice(0, bodyIndex)}${reloadClientScript}${bootstrappedHtml.slice(bodyIndex)}`);
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
    const body = injectDevRuntime(resolved.filePath, raw);
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

async function watchFrontend() {
    // The native Windows backend excludes last-access notifications.
    const watcher = await subscribe(frontendRoot, (error, events) => {
        if (error) {
            console.error(error);
            void shutdown(1);
            return;
        }
        if (events.some(event => !isRspackOwned(event.path))) {
            scheduleReload();
        }
    });

    return () => watcher.unsubscribe();
}

function rememberBundleDependencies(stats) {
    bundledFiles.clear();
    for (const child of stats.stats) {
        for (const file of child.compilation.fileDependencies) {
            bundledFiles.add(path.resolve(file));
        }
    }
}

function closeRspack(compiler, watcher) {
    return new Promise((resolve) => {
        watcher.close(() => compiler.close(resolve));
    });
}

async function startBundleWatch() {
    const compiler = rspack(rspackConfigs);
    let initialBuild = true;
    let watcher;

    const ready = new Promise((resolve, reject) => {
        watcher = compiler.watch({}, (error, stats) => {
            if (error) {
                if (initialBuild) {
                    reject(error);
                } else {
                    console.error(error);
                    void shutdown(1);
                }
                return;
            }

            const report = stats.toString({
                colors: Boolean(process.stdout.isTTY),
                preset: 'errors-warnings',
            });
            if (report) {
                console.log(report);
            }

            if (stats.hasErrors()) {
                if (initialBuild) {
                    reject(new Error('Initial frontend bundle build failed.'));
                }
                return;
            }

            rememberBundleDependencies(stats);
            if (initialBuild) {
                initialBuild = false;
                resolve();
            } else {
                scheduleReload();
            }
        });
    });

    try {
        await ready;
    } catch (error) {
        await closeRspack(compiler, watcher);
        throw error;
    }

    return () => closeRspack(compiler, watcher);
}

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
    console.error(error);
    void shutdown(1);
});

async function shutdown(exitCode = 0) {
    if (shuttingDown) {
        return;
    }
    shuttingDown = true;

    await closeFrontendWatcher();
    clearTimeout(reloadTimer);
    for (const client of reloadClients) {
        client.end();
    }
    reloadClients.clear();
    await new Promise(resolve => server.close(resolve));
    await closeBundleWatcher();
    process.exit(exitCode);
}

process.on('SIGINT', () => void shutdown());
process.on('SIGTERM', () => void shutdown());

async function main() {
    closeBundleWatcher = await startBundleWatch();
    try {
        closeFrontendWatcher = await watchFrontend();
    } catch (error) {
        await closeBundleWatcher();
        throw error;
    }
    server.listen(port, () => {
        console.log(`TauriTavern frontend dev server listening on http://localhost:${port}`);
        console.log(`TauriTavern reload endpoint advertised as ${reloadUrl}`);
    });
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
