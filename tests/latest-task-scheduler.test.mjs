import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLatestTaskScheduler } from '../src/scripts/util/latest-task-scheduler.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function deferred() {
    let resolve;
    const promise = new Promise(resolvePromise => {
        resolve = resolvePromise;
    });
    return { promise, resolve };
}

async function flushMicrotasks() {
    await Promise.resolve();
    await Promise.resolve();
}

test('requests during a running task coalesce into one latest rerun', async () => {
    const runs = [];
    const first = deferred();
    const second = deferred();
    const schedule = createLatestTaskScheduler(async () => {
        const run = runs.length;
        runs.push(run);
        await [first, second][run].promise;
    });

    schedule();
    schedule();
    schedule();
    assert.deepEqual(runs, [0]);

    first.resolve();
    await flushMicrotasks();
    assert.deepEqual(runs, [0, 1]);

    second.resolve();
    await flushMicrotasks();
    assert.deepEqual(runs, [0, 1]);
});

test('a request during the latest rerun schedules one more pass', async () => {
    const gates = [deferred(), deferred(), deferred()];
    let runCount = 0;
    const schedule = createLatestTaskScheduler(async () => {
        await gates[runCount++].promise;
    });

    schedule();
    schedule();
    gates[0].resolve();
    await flushMicrotasks();
    schedule();
    gates[1].resolve();
    await flushMicrotasks();

    assert.equal(runCount, 3);
    gates[2].resolve();
    await flushMicrotasks();
});

test('task failures are reported without wedging future requests', async () => {
    const errors = [];
    let runCount = 0;
    const schedule = createLatestTaskScheduler(async () => {
        runCount += 1;
        if (runCount === 1) throw new Error('expected failure');
    }, error => errors.push(error));

    schedule();
    await flushMicrotasks();
    schedule();
    await flushMicrotasks();

    assert.equal(runCount, 2);
    assert.equal(errors.length, 1);
    assert.match(errors[0].message, /expected failure/);
});

test('PromptManager keeps dry-run rendering wired through the latest-task scheduler', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/PromptManager.js'), 'utf8');
    const dryRunStart = source.indexOf('    async #renderAfterTryGenerate() {');
    const dryRunEnd = source.indexOf('    async #renderWithoutTryGenerate() {', dryRunStart);
    const dryRunSource = source.slice(dryRunStart, dryRunEnd);
    const renderStart = source.indexOf('    render(afterTryGenerate = true) {');
    const renderEnd = source.indexOf('    updatePromptWithPromptEditForm(', renderStart);
    const renderSource = source.slice(renderStart, renderEnd);

    assert.match(source, /import \{ createLatestTaskScheduler \} from '\.\/util\/latest-task-scheduler\.js';/);
    assert.match(
        source,
        /this\.renderDryRunLatest = createLatestTaskScheduler\(\s*\(\) => this\.#renderAfterTryGenerate\(\)/,
    );
    assert.ok(dryRunStart >= 0 && dryRunEnd > dryRunStart);
    const clearError = dryRunSource.indexOf('this.error = null;');
    const tryGenerate = dryRunSource.indexOf('await this.tryGenerate();');
    const exposeError = dryRunSource.indexOf('this.error = error instanceof Error');
    const rethrowError = dryRunSource.indexOf('throw error;', exposeError);
    const renderUi = dryRunSource.indexOf('await this.#renderPromptManagerUi();', rethrowError);
    assert.ok(clearError >= 0 && clearError < tryGenerate);
    assert.ok(tryGenerate < exposeError && exposeError < rethrowError);
    assert.ok(rethrowError < renderUi);
    assert.match(dryRunSource, /String\(error \|\| t`Unknown error`\)/);
    assert.ok(renderStart >= 0 && renderEnd > renderStart);
    assert.match(renderSource, /if \(afterTryGenerate === true\) \{\s*this\.renderDryRunLatest\(\);\s*return;/);
    assert.match(renderSource, /void this\.#renderWithoutTryGenerate\(\)\.catch/);
});
