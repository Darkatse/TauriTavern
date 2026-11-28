import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createChatDomAdapter } = await importFrontend('tauri/main/adapters/chat-surface/chat-dom-adapter.js');
const { createChatScrollAdapter } = await importFrontend('tauri/main/adapters/chat-surface/chat-scroll-adapter.js');
const { createChatSurfaceController } = await importFrontend('tauri/main/services/chat-surface/chat-surface-controller.js');
const { createChatSurfaceParticipantRegistry } = await importFrontend('tauri/main/services/chat-surface/participant-registry.js');

const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));

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
    expect(fixture.controller.getMountedMessageIds()).toEqual([0, 1, 4]);
    expect(fixture.root.querySelector('.mes.last_mes')?.getAttribute('mesid')).toBe('4');
    expect(fixture.materializeCount()).toBe(3);
    const elementOne = fixture.controller.getMessageElement(1);
    const mountOne = mountSignals.get(1);
    const contentOne = contentSignals.get(1);

    project(fixture.controller, [1, 2, 4]);
    expect(fixture.controller.getMessageElement(1)).toBe(elementOne);
    expect(calls.some(call => call.startsWith('mount-cleanup:0:true'))).toBe(true);

    const liveContent = elementOne.querySelector('.mes_text');
    const staged = createStagedContent('updated', true);
    fixture.controller.updateContent(elementOne, {
        content: staged,
        commit() {
            liveContent.replaceChildren(...staged.childNodes);
            return liveContent;
        },
    });
    expect(mountOne.aborted).toBe(false);
    expect(contentOne.aborted).toBe(true);
    expect(calls.some(call => call.startsWith('content-cleanup:1:true'))).toBe(true);
    expect(calls.some(call => call.startsWith('runtime-cleanup:1:true'))).toBe(true);

    project(fixture.controller, [2, 4]);
    expect(mountOne.aborted).toBe(true);
    project(fixture.controller, [1, 2, 4]);
    expect(fixture.controller.getMessageElement(1)).not.toBe(elementOne);
    fixture.controller.resetEpoch();
    expect(fixture.controller.getMountedMessageIds()).toEqual([]);
});

test('connected hook failure faults the epoch and reset releases the committed root', () => {
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
    expect(() => reconcile(fixture.controller, [0])).toThrow(/mount failed/);
    expect(fixture.controller.snapshot().fault).toBeInstanceOf(Error);
    fixture.controller.resetEpoch();
    expect(fixture.root.querySelector('.mes')).toBeNull();
    expect(fixture.controller.snapshot().fault).toBeNull();
});

test('cleanup failure stays visible and prevents destructive reset progress', () => {
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
    expect(() => fixture.controller.resetEpoch()).toThrow(/cleanup failed/);
    expect(element.parentElement).toBe(fixture.root);
    expect(fixture.controller.snapshot().fault).toBeInstanceOf(Error);
    expect(() => fixture.controller.resetEpoch()).toThrow(/cleanup failed/);
});

test('committed projection validation catches root, mesid and content drift', () => {
    for (const corruption of ['root', 'mesid', 'content']) {
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
        expect(() => project(fixture.controller, [0])).toThrow();
        expect(fixture.controller.snapshot().fault).toBeInstanceOf(Error);
    }
});

test('DOM adapter batches projection writes and rejects unauthorized root mutations', async () => {
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
    expect(appendCount).toBe(1);

    await nextTask();
    expect(faults).toEqual([]);
    root.append(createMessageElement(21, { mes: 'external' }));
    await nextTask();
    expect(faults[0].message).toMatch(/committed DOM projection is inconsistent/);
    adapter.dispose();
});
