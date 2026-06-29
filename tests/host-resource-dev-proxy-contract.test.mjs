import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('dev service worker proxies the same browser host resources as production', async () => {
    const [sw, init] = await Promise.all([
        readRepoFile('src/tt-ext-sw.js'),
        readRepoFile('src/init.js'),
    ]);

    for (const route of [
        '/css/user.css',
        '/thumbnail',
        '/scripts/extensions/third-party/',
        '/characters/',
        '/backgrounds/',
        '/assets/',
        '/user/images/',
        '/user/files/',
        '/User Avatars/',
        '/User%20Avatars/',
    ]) {
        assert.ok(sw.includes(route), `tt-ext-sw.js must proxy ${route}`);
        assert.ok(init.includes(route), `init.js fallback bridge must allow ${route}`);
    }

    assert.match(init, /prefix\.endsWith\('\/'\) && pathname\.startsWith\(prefix\)/);
    assert.match(sw, /request\.headers\.get\('range'\)/);
    assert.match(sw, /range: init\.headers\?\.range \|\| null/);
    assert.match(init, /range,\s*\n\s*}/);
});
