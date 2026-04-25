import test from 'node:test';
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const LOCALES_DIR = path.join(REPO_ROOT, 'src', 'locales');

const ATTACH_EXISTING_ENGLISH_I18N_KEYS = [
    'Attach to existing message',
    'Relative (to other prompts in prompt manager), In-chat @ Depth, or attached to an existing message.',
    'Attach to Role',
    'Select the role of the message you want to target.',
    'Message Index',
    'Positive: count from start (1 = first). Negative: count from end (-1 = last). 0 defaults to 1. Only counts messages of selected role.',
    'Attach Side',
    'Append',
    'Prepend',
    'Prepend to the start or append to the end of the message content.',
];

const REMOVED_PROMPT_MANAGER_ATTACH_KEYS = [
    'prompt_manager_attach_existing',
    'prompt_manager_position_help',
    'prompt_manager_attach_role',
    'prompt_manager_attach_role_help',
    'prompt_manager_attach_index',
    'prompt_manager_attach_index_help',
    'prompt_manager_attach_side',
    'prompt_manager_attach_append',
    'prompt_manager_attach_prepend',
    'prompt_manager_attach_side_help',
];

test('prompt manager attach-existing form uses English text i18n keys for new strings', async () => {
    const html = await readFile(path.join(REPO_ROOT, 'src', 'index.html'), 'utf8');

    assert.match(html, /data-i18n="Attach to existing message"/);
    assert.match(html, /data-i18n="Relative \(to other prompts in prompt manager\), In-chat @ Depth, or attached to an existing message\."/);
    assert.match(html, /<option data-i18n="User" value="user">User<\/option>/);
    assert.match(html, /<option data-i18n="AI Assistant" value="assistant">AI Assistant<\/option>/);
    assert.match(html, /<option data-i18n="System" value="system">System<\/option>/);
    assert.match(html, /<option data-i18n="Append" value="end">Append<\/option>/);
    assert.match(html, /<option data-i18n="Prepend" value="start">Prepend<\/option>/);

    for (const key of REMOVED_PROMPT_MANAGER_ATTACH_KEYS) {
        assert.doesNotMatch(html, new RegExp(`data-i18n="${key}"`));
    }
});

test('prompt manager attach-existing translations are only required for Chinese locales', async () => {
    for (const fileName of ['zh-cn.json', 'zh-tw.json']) {
        const locale = JSON.parse(await readFile(path.join(LOCALES_DIR, fileName), 'utf8'));
        const missingKeys = ATTACH_EXISTING_ENGLISH_I18N_KEYS.filter(key => !Object.hasOwn(locale, key));

        assert.deepEqual(missingKeys, [], `${fileName} is missing attach-existing i18n keys`);
    }
});

test('prompt manager attach-existing does not add new stable keys to locale files', async () => {
    const localeFiles = (await readdir(LOCALES_DIR))
        .filter(fileName => fileName.endsWith('.json') && fileName !== 'lang.json')
        .sort();

    for (const fileName of localeFiles) {
        const locale = JSON.parse(await readFile(path.join(LOCALES_DIR, fileName), 'utf8'));
        const stableKeys = REMOVED_PROMPT_MANAGER_ATTACH_KEYS.filter(key => Object.hasOwn(locale, key));

        assert.deepEqual(stableKeys, [], `${fileName} should use English text keys for attach-existing i18n`);
    }
});
