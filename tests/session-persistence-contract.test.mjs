import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('session state is flushed when the app is backgrounded or closed', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /function flushSessionState\(\)/);
    assert.match(source, /cancelDebounce\(saveSettingsDebounced\)/);
    assert.match(source, /window\.addEventListener\('pagehide', flushSessionState\)/);
    assert.match(source, /document\.visibilityState === 'hidden'/);
    assert.match(source, /flushSessionState\(\);\s*\n\s*cancelTtsPlay\(\)/);
});

test('character selection persists in the core selection flow without relying on a click handler', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const selectionFlow = source.match(/export async function selectCharacterById\([\s\S]*?\n\}/);

    assert.ok(selectionFlow, 'core character selection flow not found');
    assert.match(selectionFlow[0], /setActiveCharacter\(characters\[id\]\)/);
    assert.match(selectionFlow[0], /await saveSettings\(\)/);
});

test('group selection persists without a debounce exit window', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/RossAscends-mods.js'), 'utf8');
    const groupHandler = source.match(/\$\(document\)\.on\('click', '\.group_select',[\s\S]*?\n\s*\}\);/);

    assert.ok(groupHandler, 'group selection handler not found');
    assert.match(groupHandler[0], /void saveSettings\(\)/);
    assert.doesNotMatch(groupHandler[0], /saveSettingsDebounced/);
});
