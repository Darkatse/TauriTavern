import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { initializeChatVirtualization } = await importFrontend('tauri/main/services/chat-surface/chat-virtualization-state.js');
const { installChatSurfaceRuntime } = await importFrontend('tauri/main/services/chat-surface/install.js');

test('bounded redraw follows the remaining messages after deletions', async () => {
    initializeChatVirtualization({ chat_virtualization_enabled: true });
    const messages = ['first', 'second', 'third'].map(mes => ({ mes }));
    const root = document.createElement('div');
    root.id = 'chat';
    root.style.cssText = 'width: 800px; height: 600px; overflow-y: auto';
    document.body.append(root);
    const surface = installChatSurfaceRuntime({
        root,
        getMessages: () => messages,
        async prepareMaterializeOptions({ messages, messageIds }) {
            return new Map(messageIds.map(id => [id, { text: messages[id].mes }]));
        },
        materializeMessage({ messageId, materializeOptions }) {
            const element = document.createElement('div');
            element.className = 'mes';
            element.setAttribute('mesid', String(messageId));
            element.style.height = '200px';
            element.innerHTML = '<div class="mes_text"></div>';
            element.firstElementChild.textContent = materializeOptions.text;
            return element;
        },
        refreshMessageDetails() {},
        syncMountedViewState() {},
        onFault(error) { throw error; },
    });
    await surface.render();

    for (const remove of [() => messages.shift(), () => messages.pop(), () => messages.pop()]) {
        remove();
        await surface.render();
        expect(surface.getMountedMessageIds()).toEqual(messages.map((_, id) => id));
        expect([...root.querySelectorAll(':scope > .mes .mes_text')].map(element => element.textContent))
            .toEqual(messages.map(message => message.mes));
    }
    surface.resetEpoch();
});
