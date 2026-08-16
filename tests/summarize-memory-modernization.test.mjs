import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const memorySource = await readFile(new URL('../src/scripts/extensions/memory/index.js', import.meta.url), 'utf8');

test('new Tauri memory settings use Main API and the Extras context branch reads the nested source', () => {
    assert.match(memorySource, /source: summary_sources\.main/);
    assert.doesNotMatch(memorySource, /extension_settings\.source/);
    assert.match(memorySource, /extension_settings\.memory\.source === summary_sources\.extras/);
});
