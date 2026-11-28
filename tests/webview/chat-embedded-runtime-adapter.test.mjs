import { afterEach, beforeEach, expect, rs, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { eventSource, event_types } = await importFrontend('scripts/events.js');
const { installFrontendSourceHandoff } = await importFrontend('tauri/main/adapters/chat-surface/frontend-source-handoff.js');
const { installChatEmbeddedRuntimeAdapters } = await importFrontend('tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js');
const { dropParkedManagedIframe, takeParkedManagedIframe } = await importFrontend('tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js');
const { createEmbeddedRuntimeManager } = await importFrontend('tauri/main/services/embedded-runtime/embedded-runtime-manager.js');

const FRONTEND_SOURCE_HANDOFF_ATTRIBUTE = 'data-tt-frontend-source-handoff';

beforeEach(() => {
    rs.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame'] });
});
afterEach(() => {
    rs.useRealTimers();
});

const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));
// Bodies arrive in memory: reading a real Response takes an engine-dependent number of tasks.
const fetched = html => ({ ok: true, blob: async () => new Blob([html], { type: 'text/html' }) });

function createManagerStub(profileConfig = { maxSoftParkedIframes: 1, softParkTtlMs: 1000 }) {
    const calls = {
        register: [],
        unregister: [],
        invalidate: [],
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
    };
}

function createChat() {
    const chat = document.createElement('div');
    chat.id = 'chat';
    document.body.append(chat);
    return chat;
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
    iframe.srcdoc = '<p>runtime</p>';
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
    const chat = createChat();
    const covered = createCoveredMessage({ mesid: '36', releaseEvent: event_types.CHAT_LOADED });
    chat.append(covered.message);
    const handle = installFrontendSourceHandoff(chat);
    let replacementPre = null;
    const lateRenderer = () => {
        const replacement = createCoveredMessage({ mesid: '36', releaseEvent: event_types.CHAT_LOADED });
        covered.message.querySelector('.mes_text').replaceWith(replacement.message.querySelector('.mes_text'));
        replacementPre = replacement.pre;
    };
    try {
        await eventSource.emit(event_types.EXTENSION_SETTINGS_LOADED);
        eventSource.on(event_types.CHAT_LOADED, lateRenderer);

        await eventSource.emit(event_types.CHAT_LOADED);
        expect(replacementPre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE)).toBe(event_types.CHAT_LOADED);
        rs.advanceTimersToNextFrame();
        expect(replacementPre.getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE)).toBeNull();
    } finally {
        eventSource.removeListener(event_types.CHAT_LOADED, lateRenderer);
        handle.dispose();
    }
});

test('chat embedded-runtime adapter restores orphaned TH-render UI, parks iframe, and invalidates slot', async () => {
    const chat = createChat();
    const { message, wrapper, iframe } = createJsrMessage({ mesid: '11', orphaned: true });
    chat.append(message);
    const manager = createManagerStub({ maxSoftParkedIframes: 1, softParkTtlMs: 1000 });
    const handle = installChatEmbeddedRuntimeAdapters({ manager });
    const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
    try {
        expect(slotId).not.toBe('');
        const button = wrapper.querySelector(':scope > .TH-collapse-code-block-button');
        expect(button).not.toBeNull();
        expect(button.classList.contains('hidden!')).toBe(true);

        iframe.remove();
        await nextTask();
        rs.advanceTimersToNextFrame();

        expect(button.classList.contains('hidden!')).toBe(false);
        expect(String(button.textContent || '').trim()).not.toBe('');
        expect(takeParkedManagedIframe(slotId)).toBe(iframe);
        expect(manager.calls.invalidate).toEqual([slotId]);
        expect(manager.calls.unregister).toEqual([]);
    } finally {
        dropParkedManagedIframe(slotId);
        handle.dispose();
    }
});

test('chat embedded-runtime adapter ignores iframe removals initiated by a managed slot', async () => {
    const chat = createChat();
    const { message, wrapper } = createJsrMessage({ mesid: '12' });
    chat.append(message);
    const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
    const handle = installChatEmbeddedRuntimeAdapters({ manager });
    try {
        const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        const slot = manager.slots.get(slotId);
        expect(slot).toBeDefined();

        slot.dehydrate('visibility');
        await nextTask();
        rs.advanceTimersToNextFrame();

        expect(wrapper.querySelector('iframe')).toBeNull();
        expect(manager.calls.invalidate).toEqual([]);
        expect(manager.calls.unregister).toEqual([]);
        expect(wrapper.dataset.ttRuntimeSlotId).toBe(slotId);
    } finally {
        handle.dispose();
    }
});

test('chat embedded-runtime adapter unregisters slots when an iframe is removed and wrapper is not orphaned', async () => {
    const chat = createChat();
    const { message, wrapper, iframe } = createJsrMessage({ mesid: '12' });
    chat.append(message);
    const manager = createManagerStub({ maxSoftParkedIframes: 0, softParkTtlMs: 0 });
    const handle = installChatEmbeddedRuntimeAdapters({ manager });
    try {
        const slotId = String(wrapper.dataset.ttRuntimeSlotId || '');
        expect(slotId).not.toBe('');

        iframe.remove();
        await nextTask();
        rs.advanceTimersToNextFrame();

        expect(manager.calls.unregister).toEqual([slotId]);
        expect(wrapper.dataset.ttRuntimeSlotId).toBeUndefined();
        expect(manager.calls.invalidate).toEqual([]);
    } finally {
        handle.dispose();
    }
});

for (const change of ['replacement', 'source attributes']) {
    test(`chat adapter captures the latest ${change} before external removal and ignores the old read`, async () => {
        const oldRead = Promise.withResolvers();
        const newRead = Promise.withResolvers();
        const nativeFetch = window.fetch.bind(window);
        rs.spyOn(window, 'fetch').mockImplementation(url => {
            if (url === 'blob:old') return oldRead.promise;
            if (url === 'blob:new') return newRead.promise;
            return nativeFetch(url);
        });
        const chat = createChat();
        const { message, wrapper, iframe } = createJsrMessage({ orphaned: true });
        iframe.removeAttribute('srcdoc');
        iframe.src = 'blob:old';
        chat.append(message);
        const manager = createEmbeddedRuntimeManager({ profile: {
            name: 'test', maxActiveWeight: 10, maxActiveIframes: 1, maxActiveSlots: 1,
            maxSoftParkedIframes: 0, softParkTtlMs: 0,
            parkWhenHiddenKinds: [], rootMargin: '0px', threshold: 0,
        } });
        const adapter = installChatEmbeddedRuntimeAdapters({ manager });
        try {
            manager.reconcile();
            const current = change === 'replacement' ? document.createElement('iframe') : iframe;
            current.src = 'blob:new';
            if (change === 'replacement') iframe.replaceWith(current);
            await nextTask();
            newRead.resolve(fetched('<p>new</p>'));
            await nextTask();
            oldRead.resolve(fetched('<p>old</p>'));
            await nextTask();
            rs.advanceTimersToNextFrame();

            current.remove();
            await nextTask();
            rs.advanceTimersToNextFrame();
            rs.advanceTimersToNextFrame();
            expect(wrapper.querySelector('iframe')).toBe(current);
            expect(await (await fetch(current.src)).text()).toBe('<p>new</p>');
        } finally {
            manager.unregister(wrapper.dataset.ttRuntimeSlotId);
            adapter.dispose();
        }
    });
}
