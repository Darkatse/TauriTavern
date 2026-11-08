import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const {
    prepareMesTextHtmlPreservingEmbeddedRuntimes,
    replaceMesTextHtmlPreservingEmbeddedRuntimes,
} = await importFrontend('tauri/main/adapters/embedded-runtime/message-render-transaction.js');

function createFrontendPre(codeText) {
    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.textContent = codeText;
    pre.append(code);
    return pre;
}

function createMessage() {
    const message = document.createElement('div');
    message.classList.add('mes');
    const mesText = document.createElement('div');
    mesText.classList.add('mes_text');
    message.append(mesText);
    return { message, mesText };
}

const escapedCode = frontend => `<pre><code>${frontend.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`;

test('render transaction preserves JS-Slash-Runner wrappers when frontend blocks are unchanged', async () => {
    const { message, mesText } = createMessage();
    document.body.append(message);
    const frontend = '<!doctype html><html><body>jsr</body></html>';
    const wrapper = document.createElement('div');
    wrapper.classList.add('TH-render');
    wrapper.append(document.createElement('iframe'), createFrontendPre(frontend));
    mesText.append(wrapper);

    replaceMesTextHtmlPreservingEmbeddedRuntimes(message, escapedCode(frontend));

    expect(message.querySelector('.tt-runtime-stash')).toBeNull();
    expect(mesText.querySelector('.TH-render')).toBe(wrapper);
    expect(wrapper.dataset.ttRuntimeMoving).toBe('1');
    await Promise.resolve();
    expect(wrapper.dataset.ttRuntimeMoving).toBeUndefined();
});

test('render transaction preserves LittleWhiteBox wrappers and finalizes the new <pre>', async () => {
    const { message, mesText } = createMessage();
    document.body.append(message);
    const frontend = '<style>.card{display:block}</style><div class="card">lwb</div>';
    const wrapper = document.createElement('div');
    wrapper.classList.add('xiaobaix-iframe-wrapper');
    wrapper.append(document.createElement('iframe'));
    const pre = createFrontendPre(frontend);
    pre.classList.add('xb-show');
    pre.dataset.xbHash = 'hash123';
    mesText.append(wrapper, pre);

    replaceMesTextHtmlPreservingEmbeddedRuntimes(message, escapedCode(frontend));

    expect(message.querySelector('.tt-runtime-stash')).toBeNull();
    expect(mesText.querySelector('.xiaobaix-iframe-wrapper')).toBe(wrapper);
    const nextPre = mesText.querySelector('pre');
    expect(nextPre).not.toBeNull();
    expect(nextPre.style.display).toBe('none');
    expect(nextPre.dataset.xbFinal).toBe('true');
    expect(nextPre.dataset.xbHash).toBe('hash123');
    expect(nextPre.classList.contains('xb-show')).toBe(false);
    expect(wrapper.dataset.ttRuntimeMoving).toBe('1');
    await Promise.resolve();
    expect(wrapper.dataset.ttRuntimeMoving).toBeUndefined();
});

test('prepared content commits participant mutations and exact staged nodes once', () => {
    const { message, mesText } = createMessage();
    const transaction = prepareMesTextHtmlPreservingEmbeddedRuntimes(message, '<pre><code>next</code></pre>');
    transaction.content.setAttribute('data-participant-prepared', 'true');
    const stagedPre = transaction.content.querySelector('pre');
    transaction.commit();

    expect(mesText.getAttribute('data-participant-prepared')).toBe('true');
    expect(mesText.querySelector('pre')).toBe(stagedPre);
    expect(() => transaction.commit()).toThrow(/already committed/);
});
