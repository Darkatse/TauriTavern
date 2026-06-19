import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('library shims expose lodash as the SillyTavern underscore ABI', async () => {
    const source = await readRepoFile('src/lib.js');
    const functionStart = source.indexOf('export function initLibraryShims()');
    assert.notEqual(functionStart, -1);

    const functionEnd = source.indexOf('\n}\n\nexport {', functionStart);
    assert.notEqual(functionEnd, -1);

    const functionSource = source.slice(functionStart, functionEnd);
    assert.match(functionSource, /window\._\s*=\s*lodash\s*;/);
    assert.match(functionSource, /ABI explicit/);
});

test('library shim ABI is documented for third-party compatibility', async () => {
    const hostContract = await readRepoFile('docs/FrontendHostContract.md');
    assert.match(hostContract, /`window\._ : lodash`/);
    assert.match(hostContract, /third-party 扩展模块加载前可用/);

    const thirdPartyState = await readRepoFile('docs/CurrentState/ThirdPartyExtensions.md');
    assert.match(thirdPartyState, /`window\._`（lodash）是正式兼容 ABI/);

    const startupState = await readRepoFile('docs/CurrentState/StartupOptimization.md');
    assert.match(startupState, /`window\._ = lodash` 是正式 ABI/);
});
