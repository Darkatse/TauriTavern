import { expect, onTestFinished, test } from '@rstest/core';
import { computeAccessibleDescription, computeAccessibleName } from 'dom-accessibility-api';
import { createFrontend, importFrontend } from '../helpers/frontend.mjs';

const { applyV8RegexTasks } = await importFrontend('scripts/tauri/regex/v8-regex-worker.js');

async function openFrontend() {
    const frontend = await createFrontend();
    const { window, load, startHost, close } = frontend;
    onTestFinished(close);
    // Run the real regex evaluator; only the worker transport is replaced.
    window.Worker = class {
        postMessage({ tasks }) {
            const result = applyV8RegexTasks(tasks, script => this.onmessage({ data: { type: 'script-start', ...script } }));
            queueMicrotask(() => this.onmessage({ data: { type: 'result', tasks: result } }));
        }
        terminate() {}
    };
    await startHost();
    const script = (await load('script.js'));
    script.reloadMarkdownProcessor();
    (await load('scripts/chats.js')).addDOMPurifyHooks();
    (await load('tauri/main/services/chat-surface/chat-virtualization-state.js'))
        .initializeChatVirtualization({ chat_virtualization_enabled: false });
    const { power_user } = (await load('scripts/power-user.js'));
    const { extension_settings } = (await load('scripts/extensions.js'));
    const formatter = (await load('scripts/st-context.js')).getContext().messageFormatter;
    return { ...frontend, script, power_user, extension_settings, formatter };
}

const message = (mes, extra = {}) => ({
    name: 'Alice', mes, is_user: false, is_system: false,
    send_date: '2026-01-01T12:00:00Z', extra,
});

test('sync and batch formatting produce the same sanitized HTML, including empty regex results', async () => {
    const { window, script, extension_settings, formatter, load } = await openFrontend();
    const { regex_placement } = (await load('scripts/extensions/regex/engine.js'));
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
    expect(bodies.map(body => body.innerHTML)).toEqual(Array.from(expected));
    expect(bodies.map(body => body.querySelector('strong')?.textContent)).toEqual(['DISPLAY', 'restored']);
    expect(bodies[0].querySelector('span')?.textContent).toBe('formatted');
    expect(bodies.every(body => !body.querySelector('script, [onclick]'))).toBeTruthy();
});

test('streaming formats each version by blocks into the HTML of the whole message', async () => {
    const { window, script, power_user, load } = await openFrontend();
    power_user.auto_fix_generated_markdown = true;
    script.chat.push(message(''));
    // Showdown numbers repeated heading ids within each block; nothing else may differ.
    const normalize = html => {
        const root = window.document.createElement('div');
        root.innerHTML = html;
        root.querySelectorAll('h1, h2, h3').forEach(heading => heading.removeAttribute('id'));
        [...root.childNodes].filter(node => node.nodeType === 3 && !node.data.trim()).forEach(node => node.remove());
        return root.innerHTML;
    };
    const replies = [
        'He nods.\n\n* first\n\n* second\n\nShe "smiles".\n\n> quoted\n\n> still quoted\n\n# Title\n\nAfter.\n\n# Title',
        'Code:\n\n```js\nconst a = 1;\n\nconst b = "x";\n```\n\nDone with `one\n\ntwo` ticks.',
        '<details><summary>Status</summary>\n\n**HP** 10\n\n</details>\n\n<div>\nopen\n\nstill open',
        '<content>\nFirst part.\n\nSecond *part\n\ncontinues* here.\n</content>\n\n<b>bold\n\nstill bold</b>\n\nx < y\n\n"q" > z',
        'Thinking aloud\n</think>\nAnswer.\n\n<!-- note\n\n-->\n\n<style>\n.a {}\n\n.b {}\n</style>\n\nAfter.',
        'See [it][ref].\n\nMore.\n\n[ref]: https://example.com',
    ];
    for (const reply of replies) {
        const format = script.createStreamingMessageFormatter();
        for (let end = reply.indexOf('\n'); ; end = reply.indexOf('\n', end + 1)) {
            const text = end === -1 ? reply : reply.slice(0, end);
            expect(normalize(format(text, 'Alice', false, false, 0).join('\n')), text).toBe(normalize(script.messageFormatting(text, 'Alice', false, false, 0)));
            if (end === -1) break;
        }
    }
});

test('streaming writes keep unchanged blocks and restore content someone else changed', async () => {
    const { window, load } = await openFrontend();
    const { createBlockContent } = (await load('scripts/tauri/message/block-content.js'));
    const element = window.document.createElement('div');
    window.document.body.append(element);
    const content = createBlockContent();
    const write = blocks => content.prepare(element, blocks)?.commit();
    write(['<p>One</p>', '<p>Two</p>']);
    const first = element.firstChild;
    write(['<p>One</p>', '<p>Two three</p>']);
    expect(element.firstChild).toBe(first);
    expect(element.innerHTML).toBe('<p>One</p>\n<p>Two three</p>');
    expect(content.prepare(element, ['<p>One</p>', '<p>Two three</p>'])).toBeNull();
    first.textContent = 'Changed';
    await Promise.resolve();
    write(['<p>One</p>', '<p>Two three four</p>']);
    expect(element.innerHTML).toBe('<p>One</p>\n<p>Two three four</p>');
});

test('refresh applies a late formatter hook to already displayed body and reasoning', async () => {
    const { window, script, formatter } = await openFrontend();
    script.chat.push(message('Answer', { reasoning: 'Thought' }));
    await script.printMessages();
    formatter.addHook(html => html + '<b>late hook</b>');
    await script.refreshChatContent();
    expect(window.document.querySelector('#chat .mes_text').textContent).toMatch(/late hook/);
    expect(window.document.querySelector('#chat .mes_reasoning').textContent).toMatch(/late hook/);
});

test('reasoning opens when content arrives and respects manual collapse during streaming', async () => {
    const { window, script, power_user, load } = await openFrontend();
    const { ReasoningHandler } = (await load('scripts/reasoning.js'));
    power_user.reasoning.auto_expand = true;
    power_user.trim_spaces = false;
    script.chat.push(message('Answer', { reasoning: '  \n ', reasoning_duration: 1000 }));
    await script.printMessages();
    const root = window.document.querySelector('#chat > .mes');
    const details = root.querySelector('.mes_reasoning_details');
    expect(details.open).toBe(false);
    const handler = new ReasoningHandler();
    handler.initHandleMessage(root);
    handler.reasoning = 'First thought';
    handler.updateDom(0);
    expect(details.open).toBe(true);
    details.open = false;
    handler.reasoning += ' continued';
    handler.updateDom(0);
    expect(details.open).toBe(false);
});

test('cancelling an empty reasoning edit discards the draft and collapses the block', async () => {
    const { window, script, load } = await openFrontend();
    (await load('scripts/reasoning.js')).initReasoning();
    script.chat.push(message('Answer', { reasoning: '' }));
    await script.printMessages();
    const root = window.document.querySelector('#chat > .mes');
    window.jQuery(root).find('.mes_reasoning_edit').trigger('click');
    root.querySelector('.reasoning_edit_textarea').value = 'unsaved thought';
    window.jQuery(root).find('.mes_reasoning_edit_cancel').trigger('click');
    expect(root.querySelector('.reasoning_edit_textarea')).toBeNull();
    expect(root.querySelector('.mes_reasoning_details').open).toBe(false);
    expect(script.chat[0].extra.reasoning).toBe('');
});

test('message context stays distinct across clones and follows message renumbering', async () => {
    const { window, script, load } = await openFrontend();
    script.chat.push(message('First'), message('Second'));
    await script.printMessages();
    const roots = [...window.document.querySelectorAll('#chat > .mes')];
    expect(roots.map(root => computeAccessibleName(root))).toEqual(['Alice #0', 'Alice #1']);
    script.chat.shift();
    script.updateViewMessageIds();
    const remaining = window.document.querySelector('#chat > .mes');
    expect(computeAccessibleName(remaining)).toBe('Alice #0');
    await script.messageEdit(0);
    const editor = remaining.querySelector('#curEditTextarea');
    expect(computeAccessibleDescription(editor)).toBe('Alice #0');
    expect(editor.value).toBe('Second');
});

test('code copying uses the shared click activation once for keyboard and direct clicks', async () => {
    const { window, script, load } = await openFrontend();
    const root = window.document.createElement('div');
    root.innerHTML = '<pre><code class="hljs">a &amp; b</code></pre>';
    window.document.body.append(root);
    const copied = [];
    window.navigator.clipboard.writeText = async text => { copied.push(text); };
    script.addCopyToCodeBlocks(root);
    script.addCopyToCodeBlocks(root);
    (await load('scripts/keyboard.js')).initKeyboard();
    const copy = root.querySelector('.code-copy');
    copy.focus();
    copy.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await Promise.resolve();
    copy.click();
    await Promise.resolve();
    expect(copied).toEqual(['a & b', 'a & b']);
});
