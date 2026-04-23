import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('deleteWorldInfo supports skipping linked character save', async () => {
    const worldInfoPath = path.join(REPO_ROOT, 'src/scripts/world-info.js');
    const source = await readFile(worldInfoPath, 'utf8');

    assert.match(
        source,
        /export\s+async\s+function\s+deleteWorldInfo\s*\(\s*worldInfoName\s*,\s*\{\s*saveLinkedCharacter\s*=\s*true\s*\}\s*=\s*\{\s*\}\s*\)/,
    );

    assert.match(
        source,
        /if\s*\(\s*saveLinkedCharacter\s*&&\s*menu_type\s*!=\s*['"]create['"]\s*\)\s*\{\s*saveCharacterDebounced\s*\(\s*\)/,
    );
});

test('character deletion deletes linked worldbook without saving character', async () => {
    const scriptPath = path.join(REPO_ROOT, 'src/script.js');
    const source = await readFile(scriptPath, 'utf8');

    assert.match(
        source,
        /deleteWorldInfo\s*\(\s*worldName\s*,\s*\{\s*saveLinkedCharacter\s*:\s*false\s*\}\s*\)/,
    );
});

