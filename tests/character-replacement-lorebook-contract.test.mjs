import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function sliceSource(source, start, end) {
    const startIndex = source.indexOf(start);
    assert.notEqual(startIndex, -1, `Missing source marker: ${start}`);
    const endIndex = source.indexOf(end, startIndex);
    assert.notEqual(endIndex, -1, `Missing source marker: ${end}`);
    return source.slice(startIndex, endIndex);
}

test('replacement completes standard import work before resolving its lorebook conflict', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const droppedFiles = sliceSource(
        source,
        'export async function processDroppedFiles',
        'async function importCharactersTags',
    );
    const tagIndex = droppedFiles.indexOf('await importCharactersTags(avatarFileNames)');
    const resolveIndex = droppedFiles.indexOf('await resolveImportedCharacterLorebookConflict');
    const selectIndex = droppedFiles.indexOf('selectImportedChar');

    assert.ok(tagIndex >= 0 && tagIndex < resolveIndex && resolveIndex < selectIndex);
    assert.doesNotMatch(droppedFiles, /return avatarFileNames/);
    assert.match(droppedFiles, /finally \{\s*resumeImportedCharacterAgentAssetQueue\(\)/);

    const importCharacter = sliceSource(source, 'async function importCharacter(file', 'async function importFromURL');
    const enqueueIndex = importCharacter.indexOf('enqueueImportedCharacterAgentAssetScan');
    const returnIndex = importCharacter.indexOf('return { avatarFileName, replaced }');
    assert.ok(enqueueIndex >= 0 && enqueueIndex < returnIndex);
    assert.doesNotMatch(importCharacter, /resolveImportedCharacterLorebookConflict/);
    assert.match(importCharacter, /if \(replacement\) \{\s*await flushWorldInfoSaves/);
    assert.doesNotMatch(importCharacter, /if \(preserveFileName\) \{\s*await flushWorldInfoSaves/);
});

test('replacement lorebook choice is token-checked and keeps copy unbound', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const popup = sliceSource(
        source,
        'async function resolveImportedCharacterLorebookConflict',
        'async function resolveCharacterLorebookConflictBeforeNewChat',
    );

    assert.match(popup, /conflict\.conflict_token/);
    assert.match(popup, /text: t`Use New`/);
    assert.match(popup, /text: t`Keep Both`/);
    assert.match(popup, /if \(currentAvailable\) \{[\s\S]*text: t`Keep Current`/);
    assert.match(popup, /defaultResult: currentAvailable \? POPUP_RESULT\.CUSTOM2 : POPUP_RESULT\.CUSTOM1/);
    assert.match(popup, /error\?\.status === 409/);

    const apply = sliceSource(
        source,
        'async function applyCharacterLorebookConflictResolution',
        'async function resolveImportedCharacterLorebookConflict',
    );
    assert.match(apply, /conflict_token: conflictToken/);
    assert.match(apply, /resolved\?\.affected_world/);
    assert.doesNotMatch(apply, /fallbackWorld|imported_world/);
    assert.match(source, /async function syncResolvedWorldInfo[\s\S]*if \(resolved\.world_written\)/);
});

test('public import helpers retain their upstream void completion contract', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const utils = await readFile(path.join(REPO_ROOT, 'src/scripts/utils.js'), 'utf8');
    const droppedFiles = sliceSource(
        script,
        'export async function processDroppedFiles',
        'async function importCharactersTags',
    );
    const externalImport = sliceSource(
        utils,
        'export async function importFromExternalUrl',
        'export const clamp =',
    );

    assert.match(script, /\* @returns \{Promise<void>\}[\s\S]*export async function processDroppedFiles/);
    assert.doesNotMatch(droppedFiles, /return avatarFileNames/);
    assert.match(externalImport, /case 'character':\s*await processDroppedFiles\(\[file\], extraData, \{ replacement \}\);\s*break;/);
    assert.doesNotMatch(externalImport, /return processDroppedFiles|return \[\]/);
});

test('replacement intent rejects non-character URL content and unsupported files', async () => {
    const script = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const utils = await readFile(path.join(REPO_ROOT, 'src/scripts/utils.js'), 'utf8');
    const importCharacter = sliceSource(script, 'async function importCharacter(file', 'async function importFromURL');
    const externalImport = sliceSource(
        utils,
        'export async function importFromExternalUrl',
        'export const clamp =',
    );

    assert.match(importCharacter, /if \(!ext[^]*if \(replacement\) \{\s*throw new Error/);
    assert.match(externalImport, /if \(replacement && customContentType !== 'character'\) \{\s*throw new Error/);
    assert.match(script, /processDroppedFiles\([^]*\{ replacement: true \}/);
    assert.match(script, /importFromExternalUrl\([^]*replacement: true/);
});
