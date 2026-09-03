import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const FRONTEND_SOURCE_HANDOFF_ATTRIBUTE = 'data-tt-frontend-source-handoff';

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

async function importStable(modulePath) {
    return import(pathToFileURL(modulePath).href);
}

function createManagerStub(profileConfig = { maxSoftParkedIframes: 1, softParkTtlMs: 1000 }) {
    const calls = {
        register: [],
        unregister: [],
        invalidate: [],
        touch: [],
    };
    const slots = new Map();

    return {
        calls,
        slots,
        profileConfig,
        register(slot) {
            calls.register.push(slot.id);
            slots.set(slot.id, slot);
            slot.element.dataset.ttRuntimeSlotId = slot.id;
            return { id: slot.id, unregister: () => this.unregister(slot.id) };
        },
        unregister(id) {
            calls.unregister.push(id);
            slots.delete(id);
        },
        invalidate(id) {
            calls.invalidate.push(id);
        },
        touch(id) {
            calls.touch.push(id);
        },
    };
}

function createJsrMessage({ mesid = '1', orphaned = false } = {}) {
    const message = document.createElement('div');
    message.classList.add('mes');
    message.setAttribute('mesid', mesid);

    const wrapper = document.createElement('div');
    wrapper.classList.add('TH-render');

    if (orphaned) {
        const button = document.createElement('div');
        button.classList.add('TH-collapse-code-block-button', 'hidden!');
        wrapper.append(button);
    }

    const iframe = document.createElement('iframe');
    iframe.src = 'blob:jsr';
    wrapper.append(iframe);

    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.textContent = 'signature';
    pre.append(code);
    wrapper.append(pre);

    message.append(wrapper);

    return { message, wrapper, iframe };
}

function createCoveredMessage({ mesid, releaseEvent }) {
    const message = document.createElement('div');
    message.classList.add('mes');
    message.setAttribute('mesid', mesid);

    const mesText = document.createElement('div');
    mesText.classList.add('mes_text');
    const pre = document.createElement('pre');
    pre.setAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE, releaseEvent);
    const code = document.createElement('code');
    code.textContent = '<html><body>card</body></html>';
    pre.append(code);
    mesText.append(pre);
    message.append(mesText);

    return { message, pre };
}







test('source handoff follows a late renderer replacement within the captured message root', async () => {
    const dom = installFakeDom();
    let handle = null;
    let lateRenderer = null;
    let eventSource = null;
    let eventTypes = null;
    try {
        const { installFrontendSourceHandoff } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/frontend-source-handoff.js'),
        );
        ({ eventSource, event_types: eventTypes } = await importStable(
            path.join(REPO_ROOT, 'src/scripts/events.js'),
        ));

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const covered = createCoveredMessage({ mesid: '36', releaseEvent: eventTypes.CHAT_LOADED });
        chat.append(covered.message);
        handle = installFrontendSourceHandoff(chat);
        await eventSource.emit(eventTypes.EXTENSION_SETTINGS_LOADED);

        let replacementPre = null;
        lateRenderer = () => {
            const replacement = createCoveredMessage({ mesid: '36', releaseEvent: eventTypes.CHAT_LOADED });
            covered.message.querySelector('.mes_text').replaceWith(replacement.message.querySelector('.mes_text'));
            replacementPre = replacement.pre;
        };
        eventSource.on(eventTypes.CHAT_LOADED, lateRenderer);

        await eventSource.emit(eventTypes.CHAT_LOADED);
        assert.equal(replacementPre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_LOADED);
        dom.flushRaf();
        assert.equal(replacementPre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
    } finally {
        if (eventSource && eventTypes && lateRenderer) {
            eventSource.removeListener(eventTypes.CHAT_LOADED, lateRenderer);
        }
        handle?.dispose();
        dom.cleanup();
    }
});



test('chat embedded-runtime adapter restores orphaned TH-render UI, parks iframe, and invalidates slot', async () => {
    const dom = installFakeDom();
    let handle = null;
    let slotId = '';
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const { message, wrapper, iframe } = createJsrMessage({ mesid: '11', orphaned: true });
        chat.append(message);

        const manager = createManagerStub({ maxSoftParkedIframes: 1, softParkTtlMs: 1000 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        assert.ok(slotId);

        const button = wrapper.querySelector(':scope > .TH-collapse-code-block-button');
        assert.ok(button);
        assert.equal(button.classList.contains('hidden!'), true);

        iframe.remove();

        const observer = dom.createdMutationObservers.at(-1);
        observer._trigger([{ target: wrapper, removedNodes: [iframe], addedNodes: [] }]);
        dom.flushRaf();

        assert.equal(button.classList.contains('hidden!'), false);
        assert.ok(String(button.textContent || '').trim());

        const parked = lot.takeParkedManagedIframe(slotId);
        assert.equal(parked, iframe);

        assert.deepEqual(manager.calls.invalidate, [slotId]);
        assert.deepEqual(manager.calls.unregister, []);
    } finally {
        const lot = await importStable(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js'),
        );
        if (slotId) {
            lot.dropParkedManagedIframe(slotId);
        }
        handle?.dispose();
        dom.cleanup();
    }
});

test('chat embedded-runtime adapter ignores iframe removals initiated by a managed slot', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const { message, wrapper, iframe } = createJsrMessage({ mesid: '12' });
        chat.append(message);

        const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        const slot = manager.slots.get(slotId);
        assert.ok(slot);

        slot.dehydrate('visibility');

        const observer = dom.createdMutationObservers.at(-1);
        // MutationObserver delivers after the managed DOM mutation at the
        // microtask checkpoint.
        queueMicrotask(() => {
            observer._trigger([{ target: wrapper, removedNodes: [iframe], addedNodes: [] }]);
        });
        dom.flushMicrotasks();
        dom.flushRaf();

        assert.equal(wrapper.querySelector('iframe'), null);
        assert.deepEqual(manager.calls.invalidate, []);
        assert.deepEqual(manager.calls.unregister, []);
        assert.equal(wrapper.dataset.ttRuntimeSlotId, slotId);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});

test('chat embedded-runtime adapter unregisters slots when an iframe is removed and wrapper is not orphaned', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const { message, wrapper, iframe } = createJsrMessage({ mesid: '12' });
        chat.append(message);

        const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        assert.ok(slotId);

        iframe.remove();

        const observer = dom.createdMutationObservers.at(-1);
        observer._trigger([{ target: wrapper, removedNodes: [iframe], addedNodes: [] }]);
        dom.flushRaf();

        assert.deepEqual(manager.calls.unregister, [slotId]);
        assert.equal(wrapper.dataset.ttRuntimeSlotId, undefined);
        assert.deepEqual(manager.calls.invalidate, []);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});
