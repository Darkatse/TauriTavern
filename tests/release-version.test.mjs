import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import test from 'node:test';

const verifier = resolve('scripts/ci/verify-release-version.mjs');
const currentVersion = JSON.parse(readFileSync('package.json', 'utf8')).version;

test('stable release verifier accepts the synchronized project version', () => {
    const result = spawnSync(process.execPath, [verifier, `v${currentVersion}`], {
        encoding: 'utf8',
    });

    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), currentVersion);
});

test('stable release verifier rejects a mismatched tag', () => {
    const result = spawnSync(process.execPath, [verifier, 'v99.99.99'], {
        encoding: 'utf8',
    });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /package\.json declares/);
});

test('stable release verifier rejects prerelease tags', () => {
    const result = spawnSync(process.execPath, [verifier, `v${currentVersion}-rc.1`], {
        encoding: 'utf8',
    });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /vMAJOR\.MINOR\.PATCH/);
});
