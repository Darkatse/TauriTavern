import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const LOCALES_DIR = path.join(REPO_ROOT, 'src', 'locales');

const LOREBOOK_CONFLICT_KEYS = [
    'World/Lorebook conflict',
    'Embedded World/Lorebook',
    'Current local World/Lorebook:',
    'Embedded World/Lorebook:',
    'The embedded World/Lorebook and linked local World/Lorebook are different.',
    'The linked local World/Lorebook file is missing. You can restore it from the embedded copy or cancel.',
    'Choose which version to keep before starting a new chat. The other version will be overwritten.',
    'Save current World/Lorebook',
    'Overwrite with embedded World/Lorebook',
    'Cancel World/Lorebook conflict',
    'Failed to resolve the World/Lorebook conflict.',
    'New chat cancelled',
    'missing',
];

async function readLocale(fileName) {
    return JSON.parse(await readFile(path.join(LOCALES_DIR, fileName), 'utf8'));
}

test('character lorebook conflict popup has Simplified and Traditional Chinese translations', async () => {
    for (const fileName of ['zh-cn.json', 'zh-tw.json']) {
        const locale = await readLocale(fileName);
        const missingKeys = LOREBOOK_CONFLICT_KEYS.filter(key => !Object.hasOwn(locale, key));

        assert.deepEqual(missingKeys, [], `${fileName} is missing lorebook conflict i18n keys`);
    }
});

test('character lorebook conflict popup keeps English source keys', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src', 'script.js'), 'utf8');

    assert.doesNotMatch(source, /t`选择当前修改的世界书`/);
    assert.doesNotMatch(source, /t`内嵌世界书覆盖`/);
    assert.doesNotMatch(source, /t`取消`/);
});
