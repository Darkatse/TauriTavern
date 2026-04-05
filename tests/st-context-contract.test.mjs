import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('SillyTavern global wiring still exposes getContext()', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /globalThis\.SillyTavern\s*=\s*\{/);
    assert.match(source, /\bgetContext\b/);
});

test('SillyTavern context contract still exposes generate + stopGeneration', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/st-context.js'), 'utf8');

    const contextStart = source.indexOf('export function getContext()');
    assert.ok(contextStart >= 0);

    const slice = source.slice(contextStart, contextStart + 4000);

    assert.match(slice, /\bgenerate\s*:\s*[^,\n]+,/);
    assert.match(slice, /\bstopGeneration\b/);
    assert.match(slice, /\beventSource\b/);
    assert.match(slice, /\beventTypes\b/);
});

test('SillyTavern context generate is wired through safeGenerate', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/st-context.js'), 'utf8');

    assert.match(source, /import\s+\{\s*createSafeGenerate\s*\}\s+from\s+'\.\/util\/safe-generate\.js';/);
    assert.match(source, /import[\s\S]*\bwaitForGenerationIdle\b[\s\S]*from\s+'\.\.\/script\.js';/);
    assert.match(source, /const generateSafely = createSafeGenerate\(\{\s*waitForIdle:\s*waitForGenerationIdle,\s*generate:\s*Generate,\s*\}\);/s);
    assert.match(source, /\bgenerate\s*:\s*generateSafely,/);
});
