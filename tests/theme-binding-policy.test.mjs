import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { resolveThemeBinding } from '../src/scripts/theme-binding-policy.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('theme binding resolves chat, entity, then fallback and reports missing references', () => {
    const candidates = [
        { scope: 'chat', name: 'Missing chat theme' },
        { scope: 'character', name: 'Character theme' },
        { scope: 'fallback', name: 'Fallback theme' },
    ];

    assert.deepEqual(resolveThemeBinding(candidates, ['Character theme', 'Fallback theme']), {
        selected: candidates[1],
        missing: [candidates[0]],
    });
    assert.deepEqual(resolveThemeBinding(candidates, ['Fallback theme']), {
        selected: candidates[2],
        missing: candidates.slice(0, 2),
    });
    assert.deepEqual(resolveThemeBinding(candidates, []), {
        selected: null,
        missing: candidates,
    });
});

test('theme binding wiring uses canonical stores and context lifecycle events', async () => {
    const [powerUser, groupChats, html] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/scripts/power-user.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/index.html'), 'utf8'),
    ]);

    assert.match(powerUser, /chat_metadata\.theme/);
    assert.match(powerUser, /power_user\.theme_bindings/);
    assert.match(powerUser, /await saveMetadata\(\)/);
    assert.match(powerUser, /!await saveSettings\(\)/);
    assert.match(powerUser, /eventSource\.on\(event_types\.CHAT_CHANGED, syncThemeForCurrentContext\)/);
    assert.match(powerUser, /if \(power_user\.theme !== resolution\.selected\.name\)/);

    const selector = html.indexOf('id="themes"');
    const bindings = html.indexOf('id="theme_bindings"');
    const themeElements = html.indexOf('name="themeElements"');
    assert.ok(selector < bindings && bindings < themeElements);

    const saveStart = powerUser.indexOf('async function saveTheme(');
    const saveEnd = powerUser.indexOf('export function getThemeObject', saveStart);
    assert.ok(saveStart > -1 && saveEnd > saveStart);
    assert.doesNotMatch(powerUser.slice(saveStart, saveEnd), /power_user\.theme\s*=/);

    const deleteGroupStart = groupChats.indexOf('async function deleteGroup(id)');
    const deleteGroupEnd = groupChats.indexOf('export async function editGroup', deleteGroupStart);
    const deleteGroup = groupChats.slice(deleteGroupStart, deleteGroupEnd);
    assert.ok(deleteGroupStart > -1 && deleteGroupEnd > deleteGroupStart);
    assert.ok(deleteGroup.indexOf('selected_group = null') < deleteGroup.indexOf('event_types.CHAT_CHANGED'));
});
