import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function createFrontendPre(codeText, attrs = {}) {
    const pre = document.createElement('pre');
    for (const [name, value] of Object.entries(attrs)) {
        pre.setAttribute(name, String(value));
    }
    const code = document.createElement('code');
    code.textContent = codeText;
    pre.append(code);
    return pre;
}




test('render transaction preserves JS-Slash-Runner wrappers when frontend blocks are unchanged', async () => {
    const dom = installFakeDom();
    try {
        const { replaceMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );

        const message = document.createElement('div');
        message.classList.add('mes');
        document.body.append(message);

        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        const frontend = '<!doctype html><html><body>jsr</body></html>';
        const wrapper = document.createElement('div');
        wrapper.classList.add('TH-render');
        wrapper.append(document.createElement('iframe'));
        wrapper.append(createFrontendPre(frontend));
        mesText.append(wrapper);

        const html = `<pre><code>${frontend.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`;
        replaceMesTextHtmlPreservingEmbeddedRuntimes(message, html);

        assert.equal(message.querySelector('.tt-runtime-stash'), null);
        assert.equal(mesText.querySelector('.TH-render'), wrapper);
        assert.equal(wrapper.dataset.ttRuntimeMoving, '1');

        dom.flushMicrotasks();
        assert.equal(wrapper.dataset.ttRuntimeMoving, undefined);
    } finally {
        dom.cleanup();
    }
});


test('render transaction preserves LittleWhiteBox wrappers and finalizes the new <pre>', async () => {
    const dom = installFakeDom();
    try {
        const { replaceMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );

        const message = document.createElement('div');
        message.classList.add('mes');
        document.body.append(message);

        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        const frontend = '<style>.card{display:block}</style><div class="card">lwb</div>';
        const wrapper = document.createElement('div');
        wrapper.classList.add('xiaobaix-iframe-wrapper');
        wrapper.append(document.createElement('iframe'));

        const pre = createFrontendPre(frontend);
        pre.classList.add('xb-show');
        pre.dataset.xbHash = 'hash123';

        mesText.append(wrapper, pre);

        const html = `<pre><code>${frontend.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`;
        replaceMesTextHtmlPreservingEmbeddedRuntimes(message, html);

        assert.equal(message.querySelector('.tt-runtime-stash'), null);
        assert.equal(mesText.querySelector('.xiaobaix-iframe-wrapper'), wrapper);

        const nextPre = mesText.querySelector('pre');
        assert.ok(nextPre);
        assert.equal(nextPre.style.display, 'none');
        assert.equal(nextPre.dataset.xbFinal, 'true');
        assert.equal(nextPre.dataset.xbHash, 'hash123');
        assert.equal(nextPre.classList.contains('xb-show'), false);

        assert.equal(wrapper.dataset.ttRuntimeMoving, '1');
        dom.flushMicrotasks();
        assert.equal(wrapper.dataset.ttRuntimeMoving, undefined);
    } finally {
        dom.cleanup();
    }
});


test('prepared content commits participant mutations and exact staged nodes once', async () => {
    const dom = installFakeDom();
    try {
        const { prepareMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );
        const message = document.createElement('div');
        message.classList.add('mes');
        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        const transaction = prepareMesTextHtmlPreservingEmbeddedRuntimes(
            message,
            '<pre><code>next</code></pre>',
        );
        transaction.content.setAttribute('data-participant-prepared', 'true');
        const stagedPre = transaction.content.querySelector('pre');
        transaction.commit();

        assert.equal(mesText.getAttribute('data-participant-prepared'), 'true');
        assert.equal(mesText.querySelector('pre'), stagedPre);
        assert.throws(() => transaction.commit(), /already committed/);
    } finally {
        dom.cleanup();
    }
});
