import { expect, rs, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { installChatSurfaceApi } = await importFrontend('tauri/main/api/chat-surface.js');
const { initializeChatVirtualization } = await importFrontend('tauri/main/services/chat-surface/chat-virtualization-state.js');
const { installChatSurfaceRuntime } = await importFrontend('tauri/main/services/chat-surface/install.js');
const { getChatSurfaceContentPreparation, getInstalledChatSurfaceController } = await importFrontend('tauri/main/services/chat-surface/runtime.js');

function transaction(element, html) {
    const live = element.querySelector('.mes_text');
    const content = live.cloneNode(false);
    content.innerHTML = html;
    return {
        content,
        commit() {
            live.toggleAttribute('aria-busy', content.hasAttribute('aria-busy'));
            live.replaceChildren(...content.childNodes);
            return live;
        },
    };
}

test('prepared content survives projection changes and commits before runtime claims', async () => {
    const messages = [];
    const claims = [];
    const faults = [];
    const events = [];
    let evaluations = 0;
    let downstreamEvaluations = 0;
    let prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
    initializeChatVirtualization({ chat_virtualization_enabled: true });
    const root = document.createElement('div');
    root.id = 'chat';
    root.style.cssText = 'width: 800px; height: 600px; overflow-y: auto';
    document.body.append(root);
    const surface = installChatSurfaceRuntime({
        root,
        getMessages: () => messages,
        prepareMaterializeOptions: async ({ messages, messageIds }) => new Map(messageIds.map(id => [id, { messageHtml: messages[id].mes }])),
        formatMessageContent: message => message.mes,
        prepareContentTransaction: transaction,
        async emitEvent(...args) { events.push(args); },
        materializeMessage({ message, messageId }) {
            const element = document.createElement('div');
            element.className = 'mes';
            element.setAttribute('mesid', String(messageId));
            element.innerHTML = '<div class="mes_text"></div>';
            element.firstElementChild.innerHTML = message.mes;
            element.style.height = '200px';
            return element;
        },
        refreshMessageDetails() {},
        syncMountedViewState() {},
        onFault: error => faults.push(error),
    });
    const controller = getInstalledChatSurfaceController();
    const preparation = getChatSurfaceContentPreparation();
    const hostApi = {};
    installChatSurfaceApi(hostApi);
    const api = hostApi.chatSurface;
    const registration = api.registerContentProcessor({
        id: 'template',
        async prepare(context, renderBase) {
            evaluations++;
            return prepare(context, renderBase);
        },
    });
    api.registerContentProcessor({
        id: 'downstream',
        prepare(_context, renderBase) {
            downstreamEvaluations++;
            return renderBase();
        },
    });
    api.registerParticipant({
        id: 'runtime', protocolVersion: 1,
        prepareContent({ content }, runtimes) {
            for (const source of content.querySelectorAll('pre')) {
                claims.push(source);
                runtimes.claim(source, () => () => {});
            }
        },
    });
    const text = id => surface.getMessageElement(id)?.querySelector('.mes_text').textContent;
    const update = (id, options) => controller.updateContent(
        surface.getMessageElement(id), transaction(surface.getMessageElement(id), messages[id].mes), options,
    );

    // Cold remount and reindex reuse the result; edits and refresh recompute.
    {
        messages.push({ mes: 'first' }, { mes: 'second' });
        await surface.render();
        expect(evaluations).toBe(2);
        expect(text(0)).toBe('first');
        const previousSource = surface.getMessageElement(0).querySelector('pre');
        controller.project({ indices: [1] });
        controller.project({ indices: [0, 1] });
        await preparation.ready([0, 1]);
        expect(evaluations).toBe(2);
        expect(previousSource.isConnected).toBe(false);
        expect(claims.at(-1)).not.toBe(previousSource);
        messages.shift();
        await surface.render();
        expect(evaluations).toBe(2);
        expect(text(0)).toBe('second');
        messages[0].mes = 'edited';
        update(0);
        expect(text(0)).toBe('');
        await preparation.ready([0]);
        expect(text(0)).toBe('edited');
        expect(evaluations).toBe(3);
        await registration.refresh();
        expect(evaluations).toBe(4);
        expect(controller.getFault()).toBeNull();
    }

    // Raw processing precedes formatting and persists across remount.
    {
        prepare = async ({ message }, renderBase) => {
            message.mes = message.mes.toUpperCase();
            return `<pre>${await renderBase()}</pre>`;
        };
        messages[0].mes = '<b>raw</b>';
        update(0);
        await preparation.ready([0]);
        const count = evaluations;
        expect(text(0)).toBe('RAW');
        controller.project({ indices: [] });
        controller.project({ indices: [0] });
        await preparation.ready([0]);
        expect(text(0)).toBe('RAW');
        expect(evaluations).toBe(count);
    }

    // Content refresh preserves message roots and unsaved editors.
    {
        const element = surface.getMessageElement(0);
        controller.updateContent(element, transaction(element, '<textarea>draft</textarea>'), { transient: true });
        const editor = element.querySelector('textarea');
        editor.value = 'unsaved draft';
        const count = evaluations;
        await registration.refresh();
        expect(surface.getMessageElement(0)).toBe(element);
        expect(element.querySelector('textarea')).toBe(editor);
        expect(editor.value).toBe('unsaved draft');
        expect(evaluations).toBe(count);
        await surface.finishContent(0);
        await registration.refresh();
        expect(surface.getMessageElement(0)).toBe(element);
        expect(evaluations).toBe(count + 2);
    }

    // Repeated base reads share one downstream evaluation.
    {
        prepare = async (_context, renderBase) => {
            const first = renderBase();
            expect(renderBase()).toBe(first);
            return first;
        };
        const count = downstreamEvaluations;
        await registration.refresh();
        expect(downstreamEvaluations).toBe(count + 1);
    }

    // Message finalization emits rendered only after the prepared content is committed.
    {
        const started = Promise.withResolvers();
        const held = Promise.withResolvers();
        prepare = async (_context, renderBase) => {
            const html = await renderBase();
            started.resolve();
            await held.promise;
            return `<pre>${html}</pre>`;
        };
        messages[0].mes = 'group greeting';
        update(0);
        const eventCount = events.length;
        const finished = surface.finishContent(0, 'character_message_rendered', 'first_message');
        await started.promise;
        expect(text(0)).toBe('');
        expect(events.length).toBe(eventCount);
        held.resolve();
        await finished;
        expect(text(0)).toBe('group greeting');
        expect(events.at(-1)).toEqual(['character_message_rendered', 0, 'first_message']);
    }

    // Deleting a message cancels its pending preparation without resetting the chat.
    {
        let entered, release, signal;
        const started = new Promise(resolve => { entered = resolve; });
        const held = new Promise(resolve => { release = resolve; });
        prepare = async (context, renderBase) => {
            signal = context.signal;
            const html = await renderBase();
            entered();
            await held;
            return html;
        };
        messages[0].mes = 'deleted while preparing';
        update(0);
        const ready = preparation.ready([0]);
        await started;
        messages.shift();
        controller.reconcile({ indices: [] });
        expect(signal.aborted).toBe(true);
        release();
        await ready;
        expect(surface.getMessageElement(0)).toBeNull();
        messages.push({ mes: 'next' });
        prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
        await surface.render();
    }

    // A superseded async result cannot overwrite edits or a new chat.
    {
        let entered, release;
        let signal;
        const started = new Promise(resolve => { entered = resolve; });
        const held = new Promise(resolve => { release = resolve; });
        prepare = async (context, renderBase) => {
            signal = context.signal;
            const html = await renderBase();
            entered();
            await held;
            return `<pre>${html}</pre>`;
        };
        messages[0].mes = 'old async';
        update(0);
        const oldReady = preparation.ready([0]);
        await started;
        messages[0].mes = 'streaming';
        update(0, { transient: true });
        expect(signal.aborted).toBe(true);
        release();
        await oldReady;
        expect(text(0)).toBe('streaming');
        const count = evaluations;
        controller.project({ indices: [] });
        controller.project({ indices: [0] });
        await preparation.ready([0]);
        expect(evaluations).toBe(count);
        prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
        controller.project({ indices: [] });
        surface.setContentTransient(messages[0], false);
        controller.project({ indices: [0] });
        await preparation.ready([0]);
        expect(evaluations).toBe(count + 1);

        const enteredChat = new Promise(resolve => { entered = resolve; });
        const heldChat = new Promise(resolve => { release = resolve; });
        prepare = async (context, renderBase) => {
            signal = context.signal;
            const html = await renderBase();
            entered();
            await heldChat;
            return html;
        };
        messages[0].mes = 'departing';
        update(0);
        const departingReady = preparation.ready([0]);
        await enteredChat;
        surface.resetEpoch();
        messages.splice(0, messages.length, { mes: 'new chat' });
        expect(signal.aborted).toBe(true);
        release();
        await departingReady;
        prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
        await surface.render();
        expect(text(0)).toBe('new chat');
        expect(controller.getFault()).toBeNull();
    }

    // Preparation errors propagate without exposing unprocessed runtime sources.
    {
        prepare = async () => { throw new Error('template failure'); };
        messages[0].mes = '<pre>unprocessed</pre>';
        const claimCount = claims.length;
        update(0);
        await expect(preparation.ready([0])).rejects.toThrow(/content processor template failed for message 0/);
        expect(controller.getFault().cause.message).toMatch(/template failure/);
        expect(claims.length).toBe(claimCount);
        expect(text(0)).toBe('');
    }
    surface.resetEpoch();
});
