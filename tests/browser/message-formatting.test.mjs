import assert from 'node:assert/strict';
import test from 'node:test';
import { computeAccessibleDescription, computeAccessibleName } from 'dom-accessibility-api';
import { applyV8RegexTasks } from '../../src/scripts/tauri/regex/v8-regex-worker.js';
import { createBrowserRuntime } from './runtime.mjs';

async function openRuntime(t) {
    const runtime = createBrowserRuntime();
    const { window, getModule, load, startHost } = runtime;
    t.after(() => window.happyDOM.close());
    // Run the real regex evaluator; only the worker transport is replaced.
    window.Worker = class {
        postMessage({ tasks }) {
            const result = applyV8RegexTasks(tasks, script => this.onmessage({ data: { type: 'script-start', ...script } }));
            queueMicrotask(() => this.onmessage({ data: { type: 'result', tasks: result } }));
        }
        terminate() {}
    };
    await startHost();
    const script = (await load('script.js')).namespace;
    script.reloadMarkdownProcessor();
    getModule('scripts/chats.js').namespace.addDOMPurifyHooks();
    getModule('tauri/main/services/chat-surface/chat-virtualization-state.js').namespace
        .initializeChatVirtualization({ chat_virtualization_enabled: false });
    const { power_user } = getModule('scripts/power-user.js').namespace;
    const { extension_settings } = getModule('scripts/extensions.js').namespace;
    const formatter = getModule('scripts/st-context.js').namespace.getContext().messageFormatter;
    return { ...runtime, script, power_user, extension_settings, formatter };
}

const message = (mes, extra = {}) => ({
    name: 'Alice', mes, is_user: false, is_system: false,
    send_date: '2026-01-01T12:00:00Z', extra,
});

test('sync and batch formatting produce the same sanitized HTML, including empty regex results', async t => {
    const { window, script, getModule, extension_settings, formatter } = await openRuntime(t);
    const { regex_placement } = getModule('scripts/extensions/regex/engine.js').namespace;
    extension_settings.regex = [
        ['before', '/BEFORE/g', '**REGEX**'], ['erase', '/ERASE/g', ''],
    ].map(([id, findRegex, replaceString]) => ({
        id, scriptName: id, findRegex, replaceString, trimStrings: [],
        placement: [regex_placement.AI_OUTPUT], markdownOnly: true, substituteRegex: 0,
    }));
    formatter.addHook(text => text.replace('RAW', 'BEFORE'), { stage: formatter.stage.BEFORE_REGEX });
    formatter.addHook(text => text.replace('REGEX', 'DISPLAY') || '**restored**', { stage: formatter.stage.AFTER_REGEX });
    formatter.addHook(html => html + '<span onclick="bad()">formatted</span><script>bad()</script>');
    script.chat.push(message('RAW'), message('ERASE'));
    const expected = script.chat.map((item, id) => formatter.format(item.mes, item.name, false, false, id));
    await script.printMessages();
    const bodies = [...window.document.querySelectorAll('#chat > .mes .mes_text')];
    assert.deepEqual(bodies.map(body => body.innerHTML), Array.from(expected));
    assert.deepEqual(bodies.map(body => body.querySelector('strong')?.textContent), ['DISPLAY', 'restored']);
    assert.equal(bodies[0].querySelector('span')?.textContent, 'formatted');
    assert.ok(bodies.every(body => !body.querySelector('script, [onclick]')));
});

test('refresh applies a late formatter hook to already displayed body and reasoning', async t => {
    const { window, script, formatter } = await openRuntime(t);
    script.chat.push(message('Answer', { reasoning: 'Thought' }));
    await script.printMessages();
    formatter.addHook(html => html + '<b>late hook</b>');
    await script.refreshChatContent();
    assert.match(window.document.querySelector('#chat .mes_text').textContent, /late hook/);
    assert.match(window.document.querySelector('#chat .mes_reasoning').textContent, /late hook/);
});

test('reasoning opens when content arrives and respects manual collapse during streaming', async t => {
    const { window, script, getModule, power_user } = await openRuntime(t);
    const { ReasoningHandler } = getModule('scripts/reasoning.js').namespace;
    power_user.reasoning.auto_expand = true;
    power_user.trim_spaces = false;
    script.chat.push(message('Answer', { reasoning: '  \n ', reasoning_duration: 1000 }));
    await script.printMessages();
    const root = window.document.querySelector('#chat > .mes');
    const details = root.querySelector('.mes_reasoning_details');
    assert.equal(details.open, false);
    const handler = new ReasoningHandler();
    handler.initHandleMessage(root);
    handler.reasoning = 'First thought';
    handler.updateDom(0);
    assert.equal(details.open, true);
    details.open = false;
    handler.reasoning += ' continued';
    handler.updateDom(0);
    assert.equal(details.open, false);
});

test('cancelling an empty reasoning edit discards the draft and collapses the block', async t => {
    const { window, script, getModule } = await openRuntime(t);
    getModule('scripts/reasoning.js').namespace.initReasoning();
    script.chat.push(message('Answer', { reasoning: '' }));
    await script.printMessages();
    const root = window.document.querySelector('#chat > .mes');
    window.jQuery(root).find('.mes_reasoning_edit').trigger('click');
    root.querySelector('.reasoning_edit_textarea').value = 'unsaved thought';
    window.jQuery(root).find('.mes_reasoning_edit_cancel').trigger('click');
    assert.equal(root.querySelector('.reasoning_edit_textarea'), null);
    assert.equal(root.querySelector('.mes_reasoning_details').open, false);
    assert.equal(script.chat[0].extra.reasoning, '');
});

test('message context stays distinct across clones and follows message renumbering', async t => {
    const { window, script } = await openRuntime(t);
    script.chat.push(message('First'), message('Second'));
    await script.printMessages();
    const roots = [...window.document.querySelectorAll('#chat > .mes')];
    assert.deepEqual(roots.map(root => computeAccessibleName(root)), ['Alice #0', 'Alice #1']);
    script.chat.shift();
    script.updateViewMessageIds();
    const remaining = window.document.querySelector('#chat > .mes');
    assert.equal(computeAccessibleName(remaining), 'Alice #0');
    await script.messageEdit(0);
    const editor = remaining.querySelector('#curEditTextarea');
    assert.equal(computeAccessibleDescription(editor), 'Alice #0');
    assert.equal(editor.value, 'Second');
});

test('code copying uses the shared click activation once for keyboard and direct clicks', async t => {
    const { window, script, getModule } = await openRuntime(t);
    const root = window.document.createElement('div');
    root.innerHTML = '<pre><code class="hljs">a &amp; b</code></pre>';
    window.document.body.append(root);
    const copied = [];
    window.navigator.clipboard.writeText = async text => { copied.push(text); };
    script.addCopyToCodeBlocks(root);
    script.addCopyToCodeBlocks(root);
    getModule('scripts/keyboard.js').namespace.initKeyboard();
    const copy = root.querySelector('.code-copy');
    copy.focus();
    copy.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await Promise.resolve();
    copy.click();
    await Promise.resolve();
    assert.deepEqual(copied, ['a & b', 'a & b']);
});
