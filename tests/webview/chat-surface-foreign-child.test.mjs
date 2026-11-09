import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { ChatSurfaceForeignChildError } = await importFrontend('tauri/main/adapters/chat-surface/chat-dom-adapter.js');
const { initializeChatVirtualization } = await importFrontend('tauri/main/services/chat-surface/chat-virtualization-state.js');
const { installChatSurfaceRuntime } = await importFrontend('tauri/main/services/chat-surface/install.js');

test('an element an extension puts in the bounded chat stops it and is named by tag, id and classes', async () => {
    initializeChatVirtualization({ chat_virtualization_enabled: true });
    const messages = [{ mes: 'first' }];
    const faults = [];
    const root = document.createElement('div');
    root.id = 'chat';
    root.style.cssText = 'width: 800px; height: 600px; overflow-y: auto';
    document.body.append(root);
    const surface = installChatSurfaceRuntime({
        root,
        getMessages: () => messages,
        prepareMaterializeOptions: async ({ messageIds }) => new Map(messageIds.map(id => [id, {}])),
        materializeMessage({ messageId }) {
            const element = document.createElement('div');
            element.className = 'mes';
            element.setAttribute('mesid', String(messageId));
            element.style.height = '200px';
            element.innerHTML = '<div class="mes_text"></div>';
            return element;
        },
        refreshMessageDetails() {},
        syncMountedViewState() {},
        onFault: error => faults.push(error),
    });
    await surface.render();

    const indicator = document.createElement('div');
    indicator.id = 'typing_indicator';
    indicator.className = 'typing_indicator';
    root.append(indicator);
    messages.push({ mes: 'second' });

    await expect(surface.render()).rejects.toThrow(ChatSurfaceForeignChildError);
    expect(faults.map(fault => fault.selector)).toEqual(['div#typing_indicator.typing_indicator']);
});
