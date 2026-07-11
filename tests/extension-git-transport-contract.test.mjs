import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';

const REPO_ROOT = path.resolve(import.meta.dirname, '..');

test('extension Git operations do not inherit app-update GitHub stopping or fake mutation aborts', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions.js'), 'utf8');

    assert.doesNotMatch(source, /github-rate-limit-stopper\.js/);
    assert.doesNotMatch(source, /Only GitHub repositories are supported for extension installation/);

    const updateStart = source.indexOf('async function updateExtension(');
    const updateEnd = source.indexOf('\n/**', updateStart);
    assert.notEqual(updateStart, -1);
    assert.notEqual(updateEnd, -1);
    const updateSource = source.slice(updateStart, updateEnd);
    assert.doesNotMatch(updateSource, /AbortSignal|AbortController|signal\s*:/);

    const versionStart = source.indexOf('async function getExtensionVersion(');
    const versionEnd = source.indexOf('\n/**', versionStart);
    const versionSource = source.slice(versionStart, versionEnd);
    assert.match(versionSource, /signal:\s*abortController\?\.signal/);
});
