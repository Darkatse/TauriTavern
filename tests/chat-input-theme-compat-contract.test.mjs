import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('chat input overlay preserves the textarea as a direct theme layout item', async () => {
    const [scriptSource, styleSource] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/scripts/chat-input-fullscreen-editor.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/style.css'), 'utf8'),
    ]);

    assert.doesNotMatch(scriptSource, /appendChild\(sourceTextarea\)|sourceTextarea\.before\(/);
    assert.match(scriptSource, /inputHost\.appendChild\(expandButton\)/);
    assert.match(scriptSource, /getBoundingClientRect\(\)/);
    assert.doesNotMatch(styleSource, /\.tt-chat-input-shell/);
    assert.doesNotMatch(styleSource, /\.tt-chat-input[^\{]*\{[^}]*grid-area:\s*textarea\s*;/s);
});
