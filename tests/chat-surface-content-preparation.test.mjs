import assert from 'node:assert/strict';
import { register } from 'node:module';
import test from 'node:test';
import { installFakeDom } from './helpers/fake-dom.mjs';
import { initializeChatVirtualization } from '../src/tauri/main/services/chat-surface/chat-virtualization-state.js';
import { getInstalledChatSurfaceController, getChatSurfaceContentPreparation } from '../src/tauri/main/services/chat-surface/runtime.js';
import { installChatSurfaceApi } from '../src/tauri/main/api/chat-surface.js';

register(`data:text/javascript,${encodeURIComponent(`
    export function load(url, context, nextLoad) {
        return url === ${JSON.stringify(new URL('../src/lib.js', import.meta.url).href)}
            ? { format: 'module', source: "import * as core from '@tanstack/virtual-core'; export default core;", shortCircuit: true }
            : nextLoad(url, context);
    }
`)}`, import.meta.url);
const { installChatSurfaceRuntime } = await import('../src/tauri/main/services/chat-surface/install.js');

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

test('prepared content survives projection changes and commits before runtime claims', async t => {
    const dom = installFakeDom();
    const messages = [];
    const claims = [];
    const faults = [];
    const events = [];
    let evaluations = 0;
    let downstreamEvaluations = 0;
    let prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
    try {
        initializeChatVirtualization({ chat_virtualization_enabled: true });
        const root = document.createElement('div');
        root.id = 'chat';
        root._setRect({ width: 800, height: 600 });
        document.body.append(root);
        const surface = installChatSurfaceRuntime({
            root,
            getMessages: () => messages,
            prepareMaterializeOptions: async () => new Map(),
            formatMessageContent: message => message.mes,
            prepareContentTransaction: transaction,
            async emitEvent(...args) { events.push(args); },
            materializeMessage({ message, messageId }) {
                const element = document.createElement('div');
                element.className = 'mes';
                element.setAttribute('mesid', String(messageId));
                element.innerHTML = '<div class="mes_text"></div>';
                element.firstElementChild.innerHTML = message.mes;
                element._setRect({ height: 200 });
                return element;
            },
            syncMountedViewState() {},
            onFault: error => faults.push(error),
        });
        const controller = getInstalledChatSurfaceController();
        const preparation = getChatSurfaceContentPreparation();
        window.__TAURITAVERN__ = {};
        installChatSurfaceApi();
        const api = window.__TAURITAVERN__.api.chatSurface;
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

        await t.test('cold remount and reindex reuse the result; edits and refresh recompute', async () => {
            messages.push({ mes: 'first' }, { mes: 'second' });
            await surface.render();
            assert.equal(evaluations, 2);
            assert.equal(text(0), 'first');
            const previousSource = surface.getMessageElement(0).querySelector('pre');
            controller.project({ indices: [1] });
            controller.project({ indices: [0, 1] });
            await preparation.ready([0, 1]);
            assert.equal(evaluations, 2);
            assert.equal(previousSource.isConnected, false);
            assert.notEqual(claims.at(-1), previousSource);
            messages.shift();
            await surface.render();
            assert.equal(evaluations, 2);
            assert.equal(text(0), 'second');
            messages[0].mes = 'edited';
            update(0);
            assert.equal(text(0), '');
            await preparation.ready([0]);
            assert.equal(text(0), 'edited');
            assert.equal(evaluations, 3);
            await registration.refresh();
            assert.equal(evaluations, 4);
            assert.equal(controller.getFault(), null);
        });

        await t.test('raw processing precedes formatting and persists across remount', async () => {
            prepare = async ({ message }, renderBase) => {
                message.mes = message.mes.toUpperCase();
                return `<pre>${await renderBase()}</pre>`;
            };
            messages[0].mes = '<b>raw</b>';
            update(0);
            await preparation.ready([0]);
            const count = evaluations;
            assert.equal(text(0), 'RAW');
            controller.project({ indices: [] });
            controller.project({ indices: [0] });
            await preparation.ready([0]);
            assert.equal(text(0), 'RAW');
            assert.equal(evaluations, count);
        });

        await t.test('content refresh preserves message roots and unsaved editors', async () => {
            const element = surface.getMessageElement(0);
            controller.updateContent(element, transaction(element, '<textarea>draft</textarea>'), { transient: true });
            const editor = element.querySelector('textarea');
            editor.value = 'unsaved draft';
            const count = evaluations;
            await registration.refresh();
            assert.equal(surface.getMessageElement(0), element);
            assert.equal(element.querySelector('textarea'), editor);
            assert.equal(editor.value, 'unsaved draft');
            assert.equal(evaluations, count);
            await surface.finishContent(0);
            await registration.refresh();
            assert.equal(surface.getMessageElement(0), element);
            assert.equal(evaluations, count + 2);
        });

        await t.test('repeated base reads share one downstream evaluation', async () => {
            prepare = async (_context, renderBase) => {
                const first = renderBase();
                assert.equal(renderBase(), first);
                return first;
            };
            const count = downstreamEvaluations;
            await registration.refresh();
            assert.equal(downstreamEvaluations, count + 1);
        });

        await t.test('message finalization emits rendered only after the prepared content is committed', async () => {
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
            assert.equal(text(0), '');
            assert.equal(events.length, eventCount);
            held.resolve();
            await finished;
            assert.equal(text(0), 'group greeting');
            assert.deepEqual(events.at(-1), ['character_message_rendered', 0, 'first_message']);
        });

        await t.test('deleting a message cancels its pending preparation without resetting the chat', async () => {
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
            assert.equal(signal.aborted, true);
            release();
            await ready;
            assert.equal(surface.getMessageElement(0), null);
            messages.push({ mes: 'next' });
            prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
            await surface.render();
        });

        await t.test('a superseded async result cannot overwrite edits or a new chat', async () => {
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
            assert.equal(signal.aborted, true);
            release();
            await oldReady;
            assert.equal(text(0), 'streaming');
            const count = evaluations;
            controller.project({ indices: [] });
            controller.project({ indices: [0] });
            await preparation.ready([0]);
            assert.equal(evaluations, count);
            prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
            controller.project({ indices: [] });
            surface.setContentTransient(messages[0], false);
            controller.project({ indices: [0] });
            await preparation.ready([0]);
            assert.equal(evaluations, count + 1);

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
            assert.equal(signal.aborted, true);
            release();
            await departingReady;
            prepare = async (_context, renderBase) => `<pre>${await renderBase()}</pre>`;
            await surface.render();
            assert.equal(text(0), 'new chat');
            assert.equal(controller.getFault(), null);
        });

        await t.test('preparation errors propagate without exposing unprocessed runtime sources', async () => {
            prepare = async () => { throw new Error('template failure'); };
            messages[0].mes = '<pre>unprocessed</pre>';
            const claimCount = claims.length;
            update(0);
            await assert.rejects(preparation.ready([0]), /content processor template failed for message 0/);
            assert.match(controller.getFault().cause.message, /template failure/);
            assert.equal(claims.length, claimCount);
            assert.equal(text(0), '');
        });
        surface.resetEpoch();
    } finally {
        dom.cleanup();
    }
});
