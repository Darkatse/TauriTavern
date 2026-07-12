import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('chat input shell preserves the textarea named grid area used by mobile themes', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/style.css'), 'utf8');
    const shellRule = source.match(/\.tt-chat-input-shell\s*\{([\s\S]*?)\n\}/);

    assert.ok(shellRule, 'chat input shell CSS rule not found');
    assert.match(shellRule[1], /grid-area:\s*textarea\s*;/);
});
