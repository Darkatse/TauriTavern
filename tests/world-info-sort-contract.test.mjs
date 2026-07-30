import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('world info activation sorting avoids repeated sortedEntries index lookups', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/world-info.js'), 'utf8');
    const start = source.indexOf('// Sort the entries for the probability and the budget limit checks');
    const end = source.indexOf("let newContent = '';", start);
    const section = source.slice(start, end);

    assert.ok(start >= 0);
    assert.ok(end > start);
    assert.match(section, /const newEntries = \[\.\.\.activatedNow\];/);
    assert.match(section, /if \(activatedNow\.size > 1\) \{/);
    assert.match(section, /new Map\(sortedEntries\.map\(\(entry, index\) => \[entry, index\]\)\)/);
    assert.match(section, /\(sortedEntriesIndex\.get\(a\) \?\? -1\) - \(sortedEntriesIndex\.get\(b\) \?\? -1\)/);
    assert.doesNotMatch(section, /sortedEntries\.indexOf/);
});
