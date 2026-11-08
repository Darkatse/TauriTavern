import { statSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defaultSettings, defaultTheme, frontendRoot, systemExtensions } from './frontend-content.mjs';

const fixtureExtensions = fileURLToPath(new URL('./fixtures/extensions/', import.meta.url));
const THIRD_PARTY = '/scripts/extensions/third-party/';

export const CONTENT_TYPES = {
    '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json',
    '.png': 'image/png', '.svg': 'image/svg+xml', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf',
};

export function frontendFile(pathname) {
    const file = pathname.startsWith(THIRD_PARTY)
        ? path.join(fixtureExtensions, decodeURIComponent(pathname.slice(THIRD_PARTY.length)))
        : path.join(frontendRoot, decodeURIComponent(pathname));
    return statSync(file, { throwIfNoEntry: false })?.isFile() ? file : null;
}

// The test starts the host itself: without init.js, and with DOM-ready callbacks held so that evaluating the
// application's modules starts no chat or settings IO.
async function frontendPage() {
    const index = await readFile(path.join(frontendRoot, 'index.html'), 'utf8');
    const replace = (html, from, to) => {
        if (!html.includes(from)) throw new Error(`index.html no longer contains ${from}`);
        return html.replace(from, to);
    };
    const jquery = '<script src="lib/jquery-3.5.1.min.js"></script>';
    return replace(replace(index, '<script type="module" src="init.js"></script>', ''), jquery, `${jquery}\n<script>jQuery.holdReady(true);</script>`);
}

async function respond(pathname) {
    if (pathname === '/__test/frontend.html') return { type: 'text/html', body: await frontendPage() };
    if (pathname === '/__test/frontend-content.json') {
        return { type: 'application/json', body: JSON.stringify({ settings: defaultSettings, theme: defaultTheme, extensions: systemExtensions }) };
    }
    const file = frontendFile(pathname);
    return file && { type: CONTENT_TYPES[path.extname(file)] ?? 'application/octet-stream', body: await readFile(file) };
}

export function pluginTestFrontend() {
    const serve = async (request, response, next) => {
        try {
            const resource = await respond(new URL(request.url, 'http://localhost').pathname);
            if (!resource) return next();
            response.setHeader('content-type', resource.type);
            response.end(resource.body);
        } catch (error) {
            next(error);
        }
    };
    return {
        name: 'tauritavern:test-frontend',
        setup(api) {
            api.modifyRsbuildConfig(config => {
                config.server ??= {};
                config.server.setup = [config.server.setup ?? [], ({ server }) => { server.middlewares.use(serve); }].flat();
            });
        },
    };
}
