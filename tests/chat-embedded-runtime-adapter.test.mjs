import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

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

    return {
        calls,
        profileConfig,
        register(slot) {
            calls.register.push(slot.id);
            slot.element.dataset.ttRuntimeSlotId = slot.id;
            return { id: slot.id, unregister: () => this.unregister(slot.id) };
        },
        unregister(id) {
            calls.unregister.push(id);
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

test('chat embedded-runtime adapter scans messages and invalidates on placeholder click', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const { message, wrapper } = createJsrMessage({ mesid: '5' });
        chat.append(message);

        const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        assert.equal(manager.calls.register.length, 1);
        const slotId = wrapper.dataset.ttRuntimeSlotId;
        assert.ok(slotId);

        const placeholder = document.createElement('div');
        placeholder.classList.add('tt-runtime-placeholder');
        wrapper.append(placeholder);

        chat.dispatchEvent({ type: 'click', target: placeholder });
        assert.deepEqual(manager.calls.invalidate, [slotId]);
        assert.deepEqual(manager.calls.touch, []);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});

test('chat embedded-runtime adapter ignores managed iframe removals (ttRuntimeManaged)', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const { message, wrapper, iframe } = createJsrMessage({ mesid: '9' });
        chat.append(message);

        const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        assert.ok(slotId);

        iframe.dataset.ttRuntimeManaged = '1';
        iframe.remove();

        const observer = dom.createdMutationObservers.at(-1);
        observer._trigger([{ target: wrapper, removedNodes: [iframe], addedNodes: [] }]);

        assert.deepEqual(manager.calls.invalidate, []);
        assert.deepEqual(manager.calls.unregister, []);
    } finally {
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

test('chat embedded-runtime adapter removes placeholders when an iframe node is added', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installChatEmbeddedRuntimeAdapters } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
        handle = installChatEmbeddedRuntimeAdapters({ manager });

        const wrapper = document.createElement('div');
        wrapper.classList.add('TH-render');
        wrapper.dataset.ttRuntimeSlotId = 'slot:placeholder';

        const placeholder = document.createElement('div');
        placeholder.classList.add('tt-runtime-placeholder');
        const ghost = document.createElement('div');
        ghost.classList.add('tt-runtime-ghost');
        wrapper.append(placeholder, ghost);
        chat.append(wrapper);

        const iframe = document.createElement('iframe');
        wrapper.append(iframe);

        const observer = dom.createdMutationObservers.at(-1);
        observer._trigger([{ target: wrapper, removedNodes: [], addedNodes: [iframe] }]);

        assert.equal(wrapper.querySelector('.tt-runtime-placeholder'), null);
        assert.equal(wrapper.querySelector('.tt-runtime-ghost'), null);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});

