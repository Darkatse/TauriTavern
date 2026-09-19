import assert from 'node:assert/strict';
import { register } from 'node:module';
import test from 'node:test';
import { installFakeDom } from './helpers/fake-dom.mjs';
import { initializeChatVirtualization } from '../src/tauri/main/services/chat-surface/chat-virtualization-state.js';

// Use the installed virtualizer through the frontend vendor facade.
register(`data:text/javascript,${encodeURIComponent(`
    export function load(url, context, nextLoad) {
        return url === ${JSON.stringify(new URL('../src/lib.js', import.meta.url).href)}
            ? { format: 'module', source: "import * as core from '@tanstack/virtual-core'; export default core;", shortCircuit: true }
            : nextLoad(url, context);
    }
`)}`, import.meta.url);

const { installChatSurfaceRuntime } = await import('../src/tauri/main/services/chat-surface/install.js');

test('bounded redraw follows the remaining messages after deletions', async () => {
    const dom = installFakeDom();
    try {
        initializeChatVirtualization({ chat_virtualization_enabled: true });
        const messages = ['first', 'second', 'third'].map(mes => ({ mes }));
        const root = document.createElement('div');
        root.id = 'chat';
        root._setRect({ width: 800, height: 600 });
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
                element.innerHTML = '<div class="mes_text"></div>';
                element.firstElementChild.textContent = materializeOptions.text;
                element._setRect({ height: 200 });
                return element;
            },
            syncMountedViewState() {},
            onFault(error) { throw error; },
        });
        await surface.render();

        for (const remove of [() => messages.shift(), () => messages.pop(), () => messages.pop()]) {
            remove();
            await surface.render();
            assert.deepEqual(surface.getMountedMessageIds(), messages.map((_, id) => id));
            assert.deepEqual(
                [...root.querySelectorAll(':scope > .mes .mes_text')].map(element => element.textContent),
                messages.map(message => message.mes),
            );
        }
        surface.resetEpoch();
    } finally {
        dom.cleanup();
    }
});
