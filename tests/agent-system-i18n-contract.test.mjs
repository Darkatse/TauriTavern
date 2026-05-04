import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const EXTENSION_ROOT = path.join(REPO_ROOT, 'src', 'scripts', 'extensions', 'agent-system');

async function readJson(relativePath) {
    return JSON.parse(await readFile(path.join(EXTENSION_ROOT, relativePath), 'utf8'));
}

async function readDefaultMessageKeys() {
    const source = await readFile(path.join(EXTENSION_ROOT, 'src', 'i18n.js'), 'utf8');
    const block = source.match(/const DEFAULT_MESSAGES = Object\.freeze\(\{([\s\S]*?)\}\);/)?.[1] || '';
    return [...block.matchAll(/^    (\w+): /gm)].map(match => match[1]).sort();
}

test('Agent System manifest registers Chinese locale files', async () => {
    const manifest = await readJson('manifest.json');

    assert.equal(manifest.i18n?.['zh-cn'], 'i18n/zh-cn.json');
    assert.equal(manifest.i18n?.['zh-tw'], 'i18n/zh-tw.json');
});

test('Agent System Chinese locale files cover every extension i18n key', async () => {
    const defaultKeys = await readDefaultMessageKeys();

    for (const fileName of ['zh-cn.json', 'zh-tw.json']) {
        const locale = await readJson(path.join('i18n', fileName));
        const localeKeys = Object.keys(locale)
            .map(key => key.replace(/^agent_system\./, ''))
            .sort();

        assert.deepEqual(localeKeys, defaultKeys, `${fileName} must match Agent System i18n keys`);
    }
});
