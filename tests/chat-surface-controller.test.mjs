import test from 'node:test';
import assert from 'node:assert/strict';

import { installFakeDom } from './helpers/fake-dom.mjs';

import { createChatDomAdapter } from '../src/tauri/main/adapters/chat-surface/chat-dom-adapter.js';
import { createChatScrollAdapter } from '../src/tauri/main/adapters/chat-surface/chat-scroll-adapter.js';
import { createChatSurfaceController } from '../src/tauri/main/services/chat-surface/chat-surface-controller.js';
import { createChatSurfaceParticipantRegistry } from '../src/tauri/main/services/chat-surface/participant-registry.js';

function createMessageElement(messageId, message) {
    const element = document.createElement('div');
    element.classList.add('mes');
    element.setAttribute('mesid', String(messageId));

    const idDisplay = document.createElement('span');
    idDisplay.classList.add('mesIDDisplay');
    element.append(idDisplay);

    const content = document.createElement('div');
    content.classList.add('mes_text');
    const text = document.createElement('span');
    text.textContent = message.mes;
    content.append(text);
    if (message.runtime) {
        const pre = document.createElement('pre');
        const code = document.createElement('code');
        code.textContent = '<html>runtime</html>';
        pre.append(code);
        content.append(pre);
    }
    element.append(content);
    return element;
}

function createFixture({ messages, registry, guardUnauthorizedMutations = false }) {
    const root = document.createElement('div');
    root.id = 'chat';
    document.body.append(root);
    const faults = [];
    let controller;
    const adapter = createChatDomAdapter({
        root,
        guardUnauthorizedMutations,
        onUnauthorizedMutation(error) {
            faults.push(error);
            controller?.fault(error);
        },
    });
    let materializeCount = 0;
    controller = createChatSurfaceController({
        getMessages: () => messages,
        materializeMessage({ message, messageId }) {
            materializeCount += 1;
            return createMessageElement(messageId, message);
        },
        domAdapter: adapter,
        scrollAdapter: createChatScrollAdapter(root),
        participantRegistry: registry,
    });
    return { root, adapter, controller, faults, materializeCount: () => materializeCount };
}

function reconcile(controller, indices) {
    return controller.reconcile({ indices });
}

function project(controller, indices) {
    return controller.project({ indices });
}

function createStagedContent(text, runtime = false) {
    const content = document.createElement('div');
    content.classList.add('mes_text');
    const span = document.createElement('span');
    span.textContent = text;
    content.append(span);
    if (runtime) {
        content.append(document.createElement('pre'));
    }
    return content;
}

test('projection owns mount, content and runtime lifetimes across cold remounts', () => {
    const dom = installFakeDom();
    try {
        const messages = Array.from({ length: 5 }, (_value, index) => ({
            mes: `message-${index}`,
            runtime: index === 1 || index === 2,
        }));
        const registry = createChatSurfaceParticipantRegistry();
        const calls = [];
        const mountSignals = new Map();
        const contentSignals = new Map();
        registry.register({
            id: 'test/lifetimes',
            protocolVersion: 1,
            prepareContent({ mesid, content }, claims) {
                calls.push(`prepare:${mesid}`);
                const source = content.querySelector('pre');
                if (source) {
                    claims.claim(source, ({ signal }) => {
                        calls.push(`activate:${mesid}`);
                        return () => calls.push(`runtime-cleanup:${mesid}:${signal.aborted}`);
                    });
                }
            },
            didMount({ mesid, signal }) {
                mountSignals.set(mesid, signal);
                calls.push(`mount:${mesid}`);
                return () => calls.push(`mount-cleanup:${mesid}:${signal.aborted}`);
            },
            didCommitContent({ mesid, signal }) {
                contentSignals.set(mesid, signal);
                calls.push(`content:${mesid}`);
                return () => calls.push(`content-cleanup:${mesid}:${signal.aborted}`);
            },
        });

        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0, 1, 4]);
        assert.deepEqual(fixture.controller.getMountedMessageIds(), [0, 1, 4]);
        assert.equal(fixture.root.querySelector('.mes.last_mes')?.getAttribute('mesid'), '4');
        assert.equal(fixture.materializeCount(), 3);
        const elementOne = fixture.controller.getMessageElement(1);
        const mountOne = mountSignals.get(1);
        const contentOne = contentSignals.get(1);

        project(fixture.controller, [1, 2, 4]);
        assert.equal(fixture.controller.getMessageElement(1), elementOne);
        assert.ok(calls.some(call => call.startsWith('mount-cleanup:0:true')));

        const liveContent = elementOne.querySelector('.mes_text');
        const staged = createStagedContent('updated', true);
        fixture.controller.updateContent(elementOne, {
            content: staged,
            commit() {
                liveContent.replaceChildren(...staged.childNodes);
                return liveContent;
            },
        });
        assert.equal(mountOne.aborted, false);
        assert.equal(contentOne.aborted, true);
        assert.ok(calls.some(call => call.startsWith('content-cleanup:1:true')));
        assert.ok(calls.some(call => call.startsWith('runtime-cleanup:1:true')));

        project(fixture.controller, [2, 4]);
        assert.equal(mountOne.aborted, true);
        project(fixture.controller, [1, 2, 4]);
        assert.notEqual(fixture.controller.getMessageElement(1), elementOne);
        fixture.controller.resetEpoch();
        assert.deepEqual(fixture.controller.getMountedMessageIds(), []);
    } finally {
        dom.cleanup();
    }
});

test('mesid shifts cold-remount message roots instead of migrating participant state', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }, { mes: 'one' }];
        const registry = createChatSurfaceParticipantRegistry();
        let cleanupCount = 0;
        registry.register({
            id: 'test/mesid',
            protocolVersion: 1,
            didMount() {
                return () => { cleanupCount += 1; };
            },
        });
        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0, 1]);
        const oldOne = fixture.controller.getMessageElement(1);

        messages.shift();
        fixture.controller.reconcileMounted();
        assert.notEqual(fixture.controller.getMessageElement(0), oldOne);
        assert.equal(fixture.controller.getMessageElement(0).querySelector('.mes_text').textContent, 'one');
        assert.equal(cleanupCount, 2);
    } finally {
        dom.cleanup();
    }
});

test('detached phase rejects invalid roots and duplicate runtime claims', () => {
    for (const scenario of ['moved-content', 'claimed-content', 'duplicate-claim']) {
        const dom = installFakeDom();
        try {
            const messages = [{ mes: 'runtime', runtime: true }];
            const registry = createChatSurfaceParticipantRegistry();
            if (scenario === 'moved-content') {
                registry.register({
                    id: scenario,
                    protocolVersion: 1,
                    prepareContent({ content }) {
                        document.body.append(content);
                    },
                });
            } else {
                for (const id of scenario === 'duplicate-claim' ? ['first', 'second'] : [scenario]) {
                    registry.register({
                        id,
                        protocolVersion: 1,
                        prepareContent({ content }, claims) {
                            claims.claim(
                                scenario === 'claimed-content' ? content : content.querySelector('pre'),
                                () => () => {},
                            );
                        },
                    });
                }
            }
            const fixture = createFixture({ messages, registry });
            assert.throws(() => reconcile(fixture.controller, [0]));
            assert.equal(fixture.root.querySelectorAll(':scope > .mes').length, 0);
            assert.ok(fixture.controller.snapshot().fault instanceof Error);
        } finally {
            dom.cleanup();
        }
    }
});

test('connected hook failure faults the epoch and reset releases the committed root', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }];
        const registry = createChatSurfaceParticipantRegistry();
        registry.register({
            id: 'test/failing-hook',
            protocolVersion: 1,
            didMount() {
                throw new Error('mount failed');
            },
        });
        const fixture = createFixture({ messages, registry });
        assert.throws(() => reconcile(fixture.controller, [0]), /mount failed/);
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
        fixture.controller.resetEpoch();
        assert.equal(fixture.root.querySelector('.mes'), null);
        assert.equal(fixture.controller.snapshot().fault, null);
    } finally {
        dom.cleanup();
    }
});

test('cleanup failure stays visible and prevents destructive reset progress', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }];
        const registry = createChatSurfaceParticipantRegistry();
        registry.register({
            id: 'test/failing-cleanup',
            protocolVersion: 1,
            didMount() {
                return () => { throw new Error('cleanup failed'); };
            },
        });
        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0]);
        const element = fixture.controller.getMessageElement(0);
        assert.throws(() => fixture.controller.resetEpoch(), /cleanup failed/);
        assert.equal(element.parentElement, fixture.root);
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
        assert.throws(() => fixture.controller.resetEpoch(), /cleanup failed/);
    } finally {
        dom.cleanup();
    }
});

test('registry remains open for empty projections and freezes at the first non-empty projection', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }];
        const registry = createChatSurfaceParticipantRegistry();
        const calls = [];
        const registration = registry.register({
            id: 'first',
            protocolVersion: 1,
            didMount() { calls.push('first'); },
        });
        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, []);
        registry.register({
            id: 'second',
            protocolVersion: 1,
            didMount() { calls.push('second'); },
        });
        reconcile(fixture.controller, [0]);
        assert.deepEqual(calls, ['first', 'second']);
        assert.throws(() => registry.register({
            id: 'late',
            protocolVersion: 1,
            didMount() {},
        }), /before the first projection/);

        registration.fault(new Error('renderer failed'));
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
        assert.throws(() => fixture.controller.resetEpoch(), /first is faulted/);
    } finally {
        dom.cleanup();
    }
});

test('phase boundaries reject connected ownership corruption before runtime activation', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'runtime', runtime: true }];
        const registry = createChatSurfaceParticipantRegistry();
        let source;
        let activated = false;
        registry.register({
            id: 'runtime-owner',
            protocolVersion: 1,
            prepareContent({ content }, claims) {
                source = content.querySelector('pre');
                claims.claim(source, () => {
                    activated = true;
                    return () => {};
                });
            },
        });
        registry.register({
            id: 'bad-decorator',
            protocolVersion: 1,
            didMount() {
                source.remove();
            },
        });
        const fixture = createFixture({ messages, registry });
        assert.throws(() => reconcile(fixture.controller, [0]), /runtime source is not live/);
        assert.equal(activated, false);
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
    } finally {
        dom.cleanup();
    }
});

test('content replacement rotates only the content and runtime lifetimes', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'runtime', runtime: true }];
        const registry = createChatSurfaceParticipantRegistry();
        const contentSignals = [];
        const runtimeSignals = [];
        let runtimeCleanups = 0;
        let contentCleanups = 0;
        registry.register({
            id: 'test/content-update',
            protocolVersion: 1,
            prepareContent({ content }, claims) {
                const source = content.querySelector('pre');
                if (source) {
                    claims.claim(source, ({ signal }) => {
                        runtimeSignals.push(signal);
                        return () => { runtimeCleanups += 1; };
                    });
                }
            },
            didCommitContent({ signal }) {
                contentSignals.push(signal);
                return () => { contentCleanups += 1; };
            },
        });
        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0]);
        const element = fixture.controller.getMessageElement(0);
        const liveContent = element.querySelector('.mes_text');
        const staged = createStagedContent('next', true);

        fixture.controller.updateContent(element, {
            content: staged,
            commit() {
                liveContent.replaceChildren(...staged.childNodes);
                return liveContent;
            },
        });
        assert.equal(contentSignals[0].aborted, true);
        assert.equal(runtimeSignals[0].aborted, true);
        assert.equal(contentCleanups, 1);
        assert.equal(runtimeCleanups, 1);
        assert.equal(contentSignals.length, 2);
        assert.equal(runtimeSignals.length, 2);

        const transient = createStagedContent('streaming');
        fixture.controller.updateContent(element, {
            content: transient,
            commit() {
                liveContent.replaceChildren(...transient.childNodes);
                return liveContent;
            },
        }, { notifyParticipants: false });
        assert.equal(contentSignals.length, 2);
        assert.equal(runtimeSignals.length, 2);
    } finally {
        dom.cleanup();
    }
});

test('one mutation guard rejects synchronous participant reentrancy', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }, { mes: 'one' }];
        const registry = createChatSurfaceParticipantRegistry();
        let fixture;
        registry.register({
            id: 'test/reentrant-cleanup',
            protocolVersion: 1,
            didMount({ mesid }) {
                if (mesid === 0) {
                    return () => project(fixture.controller, [0]);
                }
            },
        });
        fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0, 1]);
        assert.throws(() => project(fixture.controller, [1]), /reentered during project/);
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
    } finally {
        dom.cleanup();
    }
});

test('runtime claims close with the detached phase and activation requires a synchronous disposer', () => {
    for (const scenario of ['late-claim', 'async-activation']) {
        const dom = installFakeDom();
        try {
            const messages = [{ mes: 'runtime', runtime: true }];
            const registry = createChatSurfaceParticipantRegistry();
            let savedClaims;
            let savedSource;
            registry.register({
                id: scenario,
                protocolVersion: 1,
                prepareContent({ content }, claims) {
                    const source = content.querySelector('pre');
                    if (scenario === 'late-claim') {
                        savedClaims = claims;
                        savedSource = source;
                    } else {
                        claims.claim(source, () => Promise.resolve(() => {}));
                    }
                },
            });
            const fixture = createFixture({ messages, registry });
            if (scenario === 'late-claim') {
                reconcile(fixture.controller, [0]);
                assert.throws(() => savedClaims.claim(savedSource, () => () => {}), /after prepareContent returned/);
            } else {
                assert.throws(() => reconcile(fixture.controller, [0]), /must return synchronously/);
                assert.ok(fixture.controller.snapshot().fault instanceof Error);
            }
        } finally {
            dom.cleanup();
        }
    }
});

test('managed runtime demand revokes and re-grants candidates with fresh signals', () => {
    const dom = installFakeDom();
    try {
        const messages = [
            { mes: 'runtime-0', runtime: true },
            { mes: 'runtime-1', runtime: true },
        ];
        const registry = createChatSurfaceParticipantRegistry();
        const activations = [];
        const cleanups = [];
        registry.register({
            id: 'test/managed-runtime',
            protocolVersion: 1,
            prepareContent({ mesid, content }, claims) {
                claims.claim(content.querySelector('pre'), ({ signal }) => {
                    activations.push({ mesid, signal });
                    return () => cleanups.push(mesid);
                });
            },
        });

        const fixture = createFixture({ messages, registry });
        fixture.controller.configureRuntimeAdmission({ mode: 'managed', maxActive: 1 });
        reconcile(fixture.controller, [0, 1]);
        fixture.controller.setRuntimeDemand({ messageIds: [0, 1] });
        assert.equal(activations.length, 0);
        dom.flushRaf();
        assert.deepEqual(activations.map(entry => entry.mesid), [0]);
        const firstSignal = activations[0].signal;

        fixture.controller.setRuntimeDemand({ messageIds: [1] });
        assert.equal(firstSignal.aborted, true);
        assert.deepEqual(cleanups, [0]);
        dom.flushRaf();
        fixture.controller.setRuntimeDemand({ messageIds: [0] });
        dom.flushRaf();
        assert.deepEqual(activations.map(entry => entry.mesid), [0, 1, 0]);
        assert.notEqual(activations[2].signal, firstSignal);
        assert.equal(fixture.controller.snapshot().runtime.active, 1);
    } finally {
        dom.cleanup();
    }
});

test('committed projection validation catches root, mesid and content drift', () => {
    for (const corruption of ['root', 'mesid', 'content']) {
        const dom = installFakeDom();
        try {
            const messages = [{ mes: 'zero' }];
            const fixture = createFixture({ messages, registry: createChatSurfaceParticipantRegistry() });
            reconcile(fixture.controller, [0]);
            const element = fixture.controller.getMessageElement(0);
            if (corruption === 'root') {
                fixture.controller.setMutationGuardEnabled(true);
                element.remove();
            } else if (corruption === 'mesid') {
                element.removeAttribute('mesid');
            } else {
                const replacement = createStagedContent('zero');
                element.querySelector('.mes_text').replaceWith(replacement);
            }
            assert.throws(() => project(fixture.controller, [0]));
            assert.ok(fixture.controller.snapshot().fault instanceof Error);
        } finally {
            dom.cleanup();
        }
    }
});

test('legacy external removal releases residency and later projection cold-remounts', () => {
    const dom = installFakeDom();
    try {
        const messages = [{ mes: 'zero' }];
        const registry = createChatSurfaceParticipantRegistry();
        let cleanupCount = 0;
        registry.register({
            id: 'test/legacy-removal',
            protocolVersion: 1,
            didMount() {
                return () => { cleanupCount += 1; };
            },
        });
        const fixture = createFixture({ messages, registry });
        reconcile(fixture.controller, [0]);
        const removed = fixture.controller.getMessageElement(0);
        removed.remove();
        fixture.controller.reconcileExternalRemovals([removed]);
        assert.equal(cleanupCount, 1);
        assert.deepEqual(fixture.controller.getMountedMessageIds(), []);

        project(fixture.controller, [0]);
        assert.notEqual(fixture.controller.getMessageElement(0), removed);
        assert.equal(fixture.controller.snapshot().fault, null);
    } finally {
        dom.cleanup();
    }
});

test('DOM adapter batches projection writes and rejects unauthorized root mutations', () => {
    const dom = installFakeDom();
    try {
        const root = document.createElement('div');
        document.body.append(root);
        const faults = [];
        const adapter = createChatDomAdapter({
            root,
            guardUnauthorizedMutations: true,
            onUnauthorizedMutation: error => faults.push(error),
        });
        const entries = Array.from({ length: 20 }, (_value, messageId) => ({
            messageId,
            element: createMessageElement(messageId, { mes: String(messageId) }),
        }));
        const nativeAppend = root.append.bind(root);
        let appendCount = 0;
        root.append = (...nodes) => {
            appendCount += 1;
            nativeAppend(...nodes);
        };
        adapter.commit({ removed: [], desired: entries });
        assert.equal(appendCount, 1);

        const observer = dom.createdMutationObservers[0];
        observer._trigger([{ target: root, addedNodes: entries.map(entry => entry.element), removedNodes: [] }]);
        const external = createMessageElement(21, { mes: 'external' });
        root.append(external);
        observer._trigger([{ target: root, addedNodes: [external], removedNodes: [] }]);
        assert.match(faults[0].message, /committed DOM projection is inconsistent/);
        adapter.dispose();
    } finally {
        dom.cleanup();
    }
});

test('DOM adapter forgets externally removed static roots before strict adoption', () => {
    const dom = installFakeDom();
    try {
        const root = document.createElement('div');
        document.body.append(root);
        const removals = [];
        const adapter = createChatDomAdapter({
            root,
            onUnauthorizedMutation: assert.fail,
            onExternalRemoval: elements => removals.push(...elements),
        });
        const entry = { messageId: 0, element: createMessageElement(0, { mes: 'zero' }) };
        adapter.commit({ removed: [], desired: [entry] });

        entry.element.remove();
        dom.createdMutationObservers[0]._trigger([{
            target: root,
            addedNodes: [],
            removedNodes: [entry.element],
        }]);

        assert.deepEqual(removals, [entry.element]);
        assert.doesNotThrow(() => adapter.setMutationGuardEnabled(true));
        adapter.dispose();
    } finally {
        dom.cleanup();
    }
});

test('scroll adapter is the sole native scroll write seam', () => {
    const dom = installFakeDom();
    try {
        const root = document.createElement('div');
        root.scrollTop = 10;
        root.scrollHeight = 500;
        const scrollCalls = [];
        root.scrollTo = options => scrollCalls.push(options);
        const animations = [];
        const adapter = createChatScrollAdapter(root, {
            animateTop: (top, duration) => animations.push({ top, duration }),
        });
        adapter.setTop(20);
        adapter.offsetTop(5);
        adapter.scrollTo({ top: 100, behavior: 'smooth' });
        adapter.animateTop(200, 300);
        assert.equal(root.scrollTop, 25);
        assert.deepEqual(scrollCalls, [{ top: 100, behavior: 'smooth' }]);
        assert.deepEqual(animations, [{ top: 200, duration: 300 }]);
        assert.throws(() => adapter.setTop(Number.NaN), /must be finite/);
    } finally {
        dom.cleanup();
    }
});
