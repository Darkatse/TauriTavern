import test from 'node:test';
import assert from 'node:assert/strict';

import { createResourceLease } from '../src/tauri/main/services/chat-surface/resource-lease.js';
import { createRuntimeAdmission } from '../src/tauri/main/services/chat-surface/runtime-admission.js';

function createScheduler() {
    let nextHandle = 1;
    const callbacks = new Map();
    return {
        schedule(callback) {
            const handle = nextHandle;
            nextHandle += 1;
            callbacks.set(handle, callback);
            return handle;
        },
        cancel(handle) {
            callbacks.delete(handle);
        },
        flushOne() {
            const entry = callbacks.entries().next().value;
            if (!entry) {
                return false;
            }
            const [handle, callback] = entry;
            callbacks.delete(handle);
            callback(0);
            return true;
        },
        size: () => callbacks.size,
    };
}

function createRecord(mountKey, messageId) {
    return {
        mountKey,
        messageId,
        mountLease: createResourceLease(),
        contentLease: createResourceLease(),
    };
}

test('managed RuntimeAdmission serializes grants and reuses candidates with fresh leases', () => {
    const scheduler = createScheduler();
    const activations = [];
    const cleanups = [];
    const faults = [];
    const admission = createRuntimeAdmission({
        assertCandidate() {},
        activate(record, candidate, lease) {
            activations.push({ mountKey: record.mountKey, signal: lease.signal });
            lease.add(() => cleanups.push(`${candidate.participantId}:${record.mountKey}`));
        },
        runScheduled(operation) {
            operation();
        },
        onFault(error) {
            faults.push(error);
        },
        schedule: scheduler.schedule,
        cancel: scheduler.cancel,
    });
    admission.configure('managed', { maxActive: 2 });

    const records = ['a', 'b', 'c'].map((mountKey, messageId) => createRecord(mountKey, messageId));
    admission.register(records.map(record => ({
        record,
        candidate: { participantId: 'test/runtime' },
    })));
    assert.deepEqual(admission.snapshot(), {
        ...admission.snapshot(),
        active: 0,
        pending: 3,
        scheduled: false,
    });

    admission.setDemand(['a', 'b', 'c']);
    assert.equal(scheduler.size(), 1);
    scheduler.flushOne();
    assert.equal(admission.snapshot().active, 1);
    assert.deepEqual(activations.map(entry => entry.mountKey), ['a']);
    assert.equal(scheduler.size(), 1);
    scheduler.flushOne();
    assert.equal(admission.snapshot().active, 2);
    assert.deepEqual(activations.map(entry => entry.mountKey), ['a', 'b']);
    assert.equal(scheduler.size(), 0);

    const firstASignal = activations[0].signal;
    admission.setDemand(['c']);
    assert.equal(firstASignal.aborted, true);
    assert.deepEqual(cleanups, ['test/runtime:a', 'test/runtime:b']);
    scheduler.flushOne();
    assert.equal(admission.snapshot().active, 1);
    assert.equal(activations.at(-1).mountKey, 'c');

    admission.setDemand(['a']);
    scheduler.flushOne();
    const secondA = activations.at(-1);
    assert.equal(secondA.mountKey, 'a');
    assert.notEqual(secondA.signal, firstASignal);
    assert.equal(secondA.signal.aborted, false);

    records[0].contentLease.close('content-test');
    assert.equal(secondA.signal.aborted, true);
    assert.equal(admission.snapshot().candidates.some(candidate => candidate.mountKey === 'a'), false);
    assert.deepEqual(faults, []);

    for (const record of records.slice(1)) {
        record.contentLease.close('content-test');
        record.mountLease.close('mount-test');
    }
    records[0].mountLease.close('mount-test');
    admission.resetEpoch();
    admission.dispose();
});

test('managed RuntimeAdmission suspends new grants without parking active runtimes', () => {
    const scheduler = createScheduler();
    const record = createRecord('only', 0);
    let activations = 0;
    const admission = createRuntimeAdmission({
        assertCandidate() {},
        activate(_record, _candidate, lease) {
            activations += 1;
            lease.add(() => {});
        },
        runScheduled: operation => operation(),
        onFault(error) {
            throw error;
        },
        schedule: scheduler.schedule,
        cancel: scheduler.cancel,
    });
    admission.configure('managed');
    assert.equal(admission.snapshot().maxActive, 8);
    admission.register([{ record, candidate: { participantId: 'test/runtime' } }]);
    admission.setDemand(['only'], { suspended: true });
    assert.equal(scheduler.size(), 0);
    assert.equal(activations, 0);
    admission.setDemand(['only']);
    scheduler.flushOne();
    assert.equal(activations, 1);
    admission.setDemand(['only'], { suspended: true });
    assert.equal(admission.snapshot().active, 1);

    record.contentLease.close('content-test');
    record.mountLease.close('mount-test');
    admission.resetEpoch();
    admission.dispose();
});
