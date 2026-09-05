import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MONKEY_PATCH_URL = pathToFileURL(
    path.join(REPO_ROOT, 'src/lib/structured-clone/monkey-patch.js'),
).href;

function runIsolated(source) {
    const result = spawnSync(process.execPath, ['--input-type=module', '--eval', source], {
        cwd: REPO_ROOT,
        encoding: 'utf8',
    });

    assert.equal(result.status, 0, result.stderr || result.stdout);
}

test('structuredClone compat installs only when the native function is missing', () => {
    runIsolated(`
        import assert from 'node:assert/strict';

        delete globalThis.structuredClone;
        await import(${JSON.stringify(MONKEY_PATCH_URL)});

        assert.equal(typeof globalThis.structuredClone, 'function');

        const source = {
            date: new Date('2020-01-01T00:00:00Z'),
            regexp: /tauritavern/gi,
            map: new Map([['key', 1]]),
            set: new Set([2]),
            bigint: 3n,
        };
        source.self = source;

        const clone = globalThis.structuredClone(source);
        assert.notEqual(clone, source);
        assert.equal(clone.self, clone);
        assert.equal(clone.date.toISOString(), source.date.toISOString());
        assert.equal(clone.regexp.source, source.regexp.source);
        assert.equal(clone.regexp.flags, source.regexp.flags);
        assert.equal(clone.map.get('key'), 1);
        assert.equal(clone.set.has(2), true);
        assert.equal(clone.bigint, 3n);
    `);

    runIsolated(`
        import assert from 'node:assert/strict';

        const nativeStructuredClone = (value) => value;
        globalThis.structuredClone = nativeStructuredClone;
        await import(${JSON.stringify(MONKEY_PATCH_URL)});

        assert.equal(globalThis.structuredClone, nativeStructuredClone);
    `);
});
