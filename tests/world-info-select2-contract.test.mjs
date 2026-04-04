import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('bootstrap wires World Info global selector select2 enforcer', async () => {
    const bootstrapPath = path.join(REPO_ROOT, 'src/tauri/main/bootstrap.js');
    const source = await readFile(bootstrapPath, 'utf8');

    assert.match(source, /\binstallWorldInfoGlobalSelectorSelect2Enforcer\b/);
});

