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

test('chat open events release only their own frontend source covers on the next frame', async () => {
    const dom = installFakeDom();
    let handle = null;
    let rendererListener = null;
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

        const group = createCoveredMessage({ mesid: '20', releaseEvent: eventTypes.CHAT_CHANGED });
        const character = createCoveredMessage({ mesid: '21', releaseEvent: eventTypes.CHAT_LOADED });
        chat.append(group.message, character.message);

        handle = installFrontendSourceHandoff(chat);

        rendererListener = () => queueMicrotask(() => character.pre.classList.add('hidden!'));
        eventSource.on(eventTypes.CHAT_LOADED, rendererListener);

        await eventSource.emit(eventTypes.CHAT_CHANGED, 'group-chat');
        assert.equal(group.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_CHANGED);
        assert.equal(character.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_LOADED);

        await eventSource.emit(eventTypes.CHAT_LOADED, { detail: { id: 'character-chat' } });
        assert.equal(character.pre.classList.contains('hidden!'), false);
        dom.flushMicrotasks();
        assert.equal(character.pre.classList.contains('hidden!'), true);
        assert.equal(group.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_CHANGED);
        assert.equal(character.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_LOADED);

        dom.flushRaf();
        assert.equal(group.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(character.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(character.pre.classList.contains('hidden!'), true);
    } finally {
        if (eventSource && eventTypes && rendererListener) {
            eventSource.removeListener(eventTypes.CHAT_LOADED, rendererListener);
        }
        handle?.dispose();
        dom.cleanup();
    }
});

test('frontend source release follows a JSR-like replacement of .mes_text', async () => {
    const dom = installFakeDom();
    let handle = null;
    let rendererListener = null;
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

        const covered = createCoveredMessage({ mesid: '25', releaseEvent: eventTypes.CHAT_LOADED });
        chat.append(covered.message);
        handle = installFrontendSourceHandoff(chat);

        let replacement = null;
        rendererListener = () => {
            const mesText = covered.message.querySelector('.mes_text');
            mesText.innerHTML = `<pre ${FRONTEND_SOURCE_HANDOFF_ATTRIBUTE}="${eventTypes.CHAT_LOADED}"><code>&lt;html&gt;&lt;body&gt;card&lt;/body&gt;&lt;/html&gt;</code></pre>`;
            replacement = mesText.querySelector('pre');
        };
        eventSource.on(eventTypes.CHAT_LOADED, rendererListener);

        // JSR is loaded after the host adapter; the settings event moves the host
        // listener behind the renderer before a chat is opened.
        await eventSource.emit(eventTypes.EXTENSION_SETTINGS_LOADED);
        await eventSource.emit(eventTypes.CHAT_LOADED);

        assert.equal(covered.pre.isConnected, false);
        assert.equal(replacement?.isConnected, true);
        assert.equal(
            replacement?.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE),
            eventTypes.CHAT_LOADED,
        );

        dom.flushRaf();
        assert.equal(replacement?.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(chat.querySelectorAll(`[${FRONTEND_SOURCE_HANDOFF_ATTRIBUTE}]`).length, 0);
    } finally {
        if (eventSource && eventTypes && rendererListener) {
            eventSource.removeListener(eventTypes.CHAT_LOADED, rendererListener);
        }
        handle?.dispose();
        dom.cleanup();
    }
});

test('frontend source release re-queries captured roots without adopting later messages', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installFrontendSourceHandoff } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/frontend-source-handoff.js'),
        );
        const { eventSource, event_types } = await importStable(
            path.join(REPO_ROOT, 'src/scripts/events.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const first = createCoveredMessage({ mesid: '30', releaseEvent: event_types.CHAT_CHANGED });
        chat.append(first.message);
        handle = installFrontendSourceHandoff(chat);

        await eventSource.emit(event_types.CHAT_CHANGED, 'first-chat');

        const next = createCoveredMessage({ mesid: '31', releaseEvent: event_types.CHAT_CHANGED });
        chat.append(next.message);
        dom.flushRaf();

        assert.equal(first.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(next.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), event_types.CHAT_CHANGED);

        await eventSource.emit(event_types.CHAT_CHANGED, 'next-chat');
        dom.flushRaf();
        assert.equal(next.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});

test('frontend source handoff resolves the direct message owner through nested .mes card markup', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installFrontendSourceHandoff } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/frontend-source-handoff.js'),
        );
        const { eventSource, event_types } = await importStable(
            path.join(REPO_ROOT, 'src/scripts/events.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);
        const covered = createCoveredMessage({ mesid: '32', releaseEvent: event_types.CHAT_LOADED });
        const nested = document.createElement('div');
        nested.classList.add('mes');
        covered.pre.replaceWith(nested);
        nested.append(covered.pre);
        chat.append(covered.message);
        handle = installFrontendSourceHandoff(chat);

        await eventSource.emit(event_types.CHAT_LOADED);
        dom.flushRaf();
        assert.equal(covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
    } finally {
        handle?.dispose();
        dom.cleanup();
    }
});

test('deferred extension settings keep chat-open release behind newly registered renderer listeners', async () => {
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

        const covered = createCoveredMessage({ mesid: '35', releaseEvent: eventTypes.CHAT_LOADED });
        chat.append(covered.message);
        handle = installFrontendSourceHandoff(chat);

        let markerSeenByRenderer = null;
        lateRenderer = () => {
            dom.flushRaf();
            markerSeenByRenderer = covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE);
            covered.pre.classList.add('hidden!');
        };
        eventSource.on(eventTypes.CHAT_LOADED, lateRenderer);

        await eventSource.emit(eventTypes.EXTENSION_SETTINGS_LOADED);
        await eventSource.emit(eventTypes.CHAT_LOADED);

        assert.equal(markerSeenByRenderer, eventTypes.CHAT_LOADED);
        assert.equal(covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), eventTypes.CHAT_LOADED);
        dom.flushRaf();
        assert.equal(covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(covered.pre.classList.contains('hidden!'), true);
    } finally {
        if (eventSource && eventTypes && lateRenderer) {
            eventSource.removeListener(eventTypes.CHAT_LOADED, lateRenderer);
        }
        handle?.dispose();
        dom.cleanup();
    }
});

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

test('disposing frontend source handoff synchronously uncovers pending source', async () => {
    const dom = installFakeDom();
    let handle = null;
    try {
        const { installFrontendSourceHandoff } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/frontend-source-handoff.js'),
        );
        const { eventSource, event_types } = await importStable(
            path.join(REPO_ROOT, 'src/scripts/events.js'),
        );

        const chat = document.createElement('div');
        chat.setAttribute('id', 'chat');
        document.body.append(chat);

        const covered = createCoveredMessage({ mesid: '40', releaseEvent: event_types.CHAT_LOADED });
        chat.append(covered.message);
        handle = installFrontendSourceHandoff(chat);

        await eventSource.emit(event_types.CHAT_LOADED);
        assert.equal(covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), event_types.CHAT_LOADED);

        handle.dispose();
        handle = null;
        assert.equal(covered.pre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        dom.flushRaf();
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
