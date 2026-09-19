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
