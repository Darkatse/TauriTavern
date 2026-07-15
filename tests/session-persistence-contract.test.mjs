import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('session persistence uses the host lifecycle registry and flushes pending work only', async () => {
    const scriptSource = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const contextSource = await readFile(path.join(REPO_ROOT, 'src/tauri/main/context/index.js'), 'utf8');
    const invokeSource = await readFile(path.join(REPO_ROOT, 'src/tauri/main/services/invokes/invoke-service.js'), 'utf8');

    assert.match(scriptSource, /registerLifecycleFlushHandler\('session-state', flushSessionState, \{ priority: -100 \}\)/);
    assert.match(scriptSource, /flushPendingWorldInfoSettings\(\)/);
    assert.match(scriptSource, /flushPendingSettingsSave\(worldInfoSettingsPending\)/);
    assert.match(scriptSource, /flushDebouncedChatSave\(\)/);
    assert.doesNotMatch(scriptSource, /window\.addEventListener\('pagehide', flushSessionState\)/);

    const sessionFlush = scriptSource.match(/function flushSessionState\(\) \{[\s\S]*?registerLifecycleFlushHandler\('session-state', flushSessionState, \{ priority: -100 \}\);/);
    assert.ok(sessionFlush, 'session lifecycle flush not found');
    assert.match(sessionFlush[0], /if \(saved === false\)/);
    assert.doesNotMatch(sessionFlush[0], /saveChatConditional\(\)/);
    assert.doesNotMatch(sessionFlush[0], /\bsaveSettings\(\)/);

    assert.match(contextSource, /registerLifecycleFlushHandler\('invoke-broker', invokeService\.flushAllInvokes, \{ priority: 100 \}\)/);
    assert.match(contextSource, /installLifecycleFlushHandlers\(\)/);
    assert.doesNotMatch(invokeSource, /installFlushOnHide/);
});

test('world info selection participates in the pending settings flush', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');
    const lifecycleHooks = source.match(/function installWorldInfoFlushHooks\(\) \{[\s\S]*?installWorldInfoFlushHooks\(\);/);

    assert.ok(lifecycleHooks, 'world info lifecycle hooks not found');
    assert.match(source, /export function flushPendingWorldInfoSettings\(\)/);
    assert.match(source, /registerLifecycleFlushHandler\('world-info', flushSoon\)/);
    assert.doesNotMatch(lifecycleHooks[0], /window\.addEventListener\('pagehide'/);
    assert.doesNotMatch(lifecycleHooks[0], /window\.addEventListener\('beforeunload'/);
    assert.doesNotMatch(lifecycleHooks[0], /document\.addEventListener\('visibilitychange'/);
});

test('all character chat navigation shares one debounced persistence request', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const selectionFlow = source.match(/export async function selectCharacterById\([\s\S]*?\n\}/);

    assert.ok(selectionFlow, 'core character selection flow not found');
    assert.match(source, /select_selected_character\(this_chid\);/);
    assert.doesNotMatch(source, /persistSettings/);
    assert.match(selectionFlow[0], /setActiveCharacter\(characters\[id\]\);/);
    assert.doesNotMatch(selectionFlow[0], /saveSettingsDebounced\(\)/);
    assert.doesNotMatch(selectionFlow[0], /await saveSettings\(\)/);

    const editorSelection = source.match(/export function select_selected_character\([\s\S]*?\n\}/);
    assert.ok(editorSelection, 'character editor selection flow not found');
    assert.match(editorSelection[0], /saveSettingsDebounced\(\);/);
});

test('group selection keeps one debounced persistence request', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/RossAscends-mods.js'), 'utf8');
    const groupHandler = source.match(/\$\(document\)\.on\('click', '\.group_select',[\s\S]*?\n\s*\}\);/);

    assert.ok(groupHandler, 'group selection handler not found');
    assert.match(groupHandler[0], /saveSettingsDebounced\(\)/);
    assert.doesNotMatch(groupHandler[0], /\bsaveSettings\(\)/);
});
