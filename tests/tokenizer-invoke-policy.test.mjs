import assert from 'node:assert/strict';
import test from 'node:test';

import { createInvokeBroker } from '../src/tauri/main/brokers/invoke-broker.js';
import { createHostInvokePolicies } from '../src/tauri/main/kernel/invokes/invoke-policies.js';

function deferred() {
    let resolve;
    const promise = new Promise(resolvePromise => {
        resolve = resolvePromise;
    });
    return { promise, resolve };
}

test('token prefix policy deduplicates exact requests and serializes distinct native work', async () => {
    const gates = [deferred(), deferred()];
    const calls = [];
    let active = 0;
    let maxActive = 0;
    const broker = createInvokeBroker({
        policies: createHostInvokePolicies(),
        transport: async (command, args) => {
            assert.equal(command, 'count_openai_token_prefixes');
            const index = calls.length;
            calls.push(args);
            active += 1;
            maxActive = Math.max(maxActive, active);
            await gates[index].promise;
            active -= 1;
            return { token_counts: [index + 1] };
        },
    });

    const firstArgs = { dto: { model: 'gpt-4o', base: 'a', suffixes: ['b'], stop_at: 10 } };
    const secondArgs = { dto: { model: 'gpt-4o', base: 'x', suffixes: ['y'], stop_at: 10 } };
    const first = broker.invoke('count_openai_token_prefixes', firstArgs);
    const duplicate = broker.invoke('count_openai_token_prefixes', structuredClone(firstArgs));
    const second = broker.invoke('count_openai_token_prefixes', secondArgs);

    await Promise.resolve();
    assert.equal(calls.length, 1);
    assert.equal(maxActive, 1);

    gates[0].resolve();
    assert.deepEqual(await first, { token_counts: [1] });
    assert.deepEqual(await duplicate, { token_counts: [1] });
    await Promise.resolve();
    assert.equal(calls.length, 2);
    assert.equal(maxActive, 1);

    gates[1].resolve();
    assert.deepEqual(await second, { token_counts: [2] });
    assert.deepEqual(calls, [firstArgs, secondArgs]);
});

test('token prefix policy does not cache settled results', async () => {
    let calls = 0;
    const broker = createInvokeBroker({
        policies: createHostInvokePolicies(),
        transport: async () => ({ token_counts: [++calls] }),
    });
    const args = { dto: { model: 'gpt-4o', base: 'a', suffixes: ['b'], stop_at: null } };

    assert.deepEqual(await broker.invoke('count_openai_token_prefixes', args), { token_counts: [1] });
    assert.deepEqual(await broker.invoke('count_openai_token_prefixes', args), { token_counts: [2] });
});

test('token prefix policy uses the complete DTO as its dedupe identity', () => {
    const policy = createHostInvokePolicies().count_openai_token_prefixes;
    const first = { dto: { model: 'gpt-4o', base: 'a', suffixes: ['b'], stop_at: 10 } };
    const second = { dto: { model: 'gpt-4o', base: 'a', suffixes: ['c'], stop_at: 10 } };

    assert.equal(policy.key(first), JSON.stringify(first.dto));
    assert.notEqual(policy.key(first), policy.key(second));
});
