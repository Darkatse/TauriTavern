import test from 'node:test';
import assert from 'node:assert/strict';
import { getRequiredTagLiteral } from '../src/scripts/extensions/regex/literal-gate.js';
import {
    applyV8RegexBatch,
    REGEX_EXECUTION_TIMEOUT_MS,
    V8RegexTimeoutError,
} from '../src/scripts/tauri/regex/v8-regex-worker-client.js';
import { applyV8RegexTasks } from '../src/scripts/tauri/regex/v8-regex-worker.js';

test('literal gate only accepts provable case-sensitive tag prefixes', () => {
    const tagPatterns = [
        [/<UpdateVariable>[\s\S]*?<\/UpdateVariable>/gm, '<UpdateVariable'],
        [/<StatusBlock[^>]*>[\s\S]*?<\/StatusBlock>/g, '<StatusBlock'],
        [/^.*?<\/customize_cot>/s, '</customize_cot'],
        [/<safe>.*?<\/safe>/gs, '<safe'],
        [/<宿命>[\s\S]*?<\/宿命>/gm, '<宿命'],
        [/<StatusPlaceHolderImpl\/>/g, '<StatusPlaceHolderImpl'],
        [/<Dice1\/>/g, '<Dice1'],
    ];

    for (const [regex, literal] of tagPatterns) {
        assert.equal(getRequiredTagLiteral(regex), literal);
    }

    assert.equal(getRequiredTagLiteral(/<safe>|plain/g), null);
    assert.equal(getRequiredTagLiteral(/<safe>.*?<\/safe>/gi), null);
    assert.equal(getRequiredTagLiteral(/<(safe)>/g), null);
    assert.equal(getRequiredTagLiteral(/<safe?>/g), null);
});

test('V8 worker preserves portable replacement semantics', () => {
    const starts = [];
    const script = {
        scriptKey: 'test-key',
        pattern: '(?<value>\\w+)-(?<suffix>x)',
        flags: 'g',
        requiredLiteral: '<tag',
        replacement: '$<value>:$2:$0:$$',
        trimStrings: ['a'],
    };

    const result = applyV8RegexTasks([
        { text: '<tag>foo-x</tag> <tag>bar-x</tag>', scripts: [script] },
        { text: 'no tag here', scripts: [script] },
    ], current => starts.push(current.scriptKey));

    assert.deepEqual(result, [
        { text: '<tag>foo:x:foo-x:$$</tag> <tag>br:x:br-x:$$</tag>' },
        { text: 'no tag here' },
    ]);
    assert.deepEqual(starts, ['test-key']);
});

test('V8 batches share one worker, honor allowSlow, and terminate timed out scripts', async t => {
    const previousWorker = globalThis.Worker;
    const workers = [];

    class FakeWorker {
        constructor() {
            this.onmessage = null;
            this.onerror = null;
            this.terminated = false;
            workers.push(this);
        }

        postMessage() {}

        terminate() {
            this.terminated = true;
        }
    }

    globalThis.Worker = FakeWorker;
    t.mock.timers.enable({ apis: ['setTimeout'] });
    t.after(() => {
        t.mock.timers.reset();
        if (previousWorker === undefined) {
            delete globalThis.Worker;
        } else {
            globalThis.Worker = previousWorker;
        }
    });

    const first = applyV8RegexBatch([]);
    await Promise.resolve();
    workers[0].onmessage({ data: { type: 'result', tasks: [] } });
    await first;

    const allowed = applyV8RegexBatch([]);
    await Promise.resolve();
    assert.equal(workers.length, 1);
    workers[0].onmessage({
        data: { type: 'script-start', scriptKey: 'allowed-key', allowSlow: true },
    });
    t.mock.timers.tick(REGEX_EXECUTION_TIMEOUT_MS);
    assert.equal(workers[0].terminated, false);
    workers[0].onmessage({ data: { type: 'result', tasks: [] } });
    await allowed;

    const timed = applyV8RegexBatch([]);
    const rejected = assert.rejects(timed, error => {
        assert.ok(error instanceof V8RegexTimeoutError);
        assert.equal(error.scriptKey, 'slow-key');
        return true;
    });
    await Promise.resolve();
    assert.equal(workers.length, 1);
    workers[0].onmessage({
        data: {
            type: 'script-start',
            scriptKey: 'slow-key',
            scriptName: 'Slow script',
        },
    });
    t.mock.timers.tick(REGEX_EXECUTION_TIMEOUT_MS);
    await rejected;
    assert.equal(workers[0].terminated, true);
});
