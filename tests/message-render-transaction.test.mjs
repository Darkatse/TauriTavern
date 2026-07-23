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

test('replaceMesTextHtmlPreservingEmbeddedRuntimes fails fast on invalid DOM', async () => {
    const dom = installFakeDom();
    try {
        const { replaceMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );

        assert.throws(
            () => replaceMesTextHtmlPreservingEmbeddedRuntimes(null, '<div/>'),
            /messageElement must be an HTMLElement/,
        );

        const message = document.createElement('div');
        assert.throws(
            () => replaceMesTextHtmlPreservingEmbeddedRuntimes(message, '<div/>'),
            /\.mes_text not found/,
        );
    } finally {
        dom.cleanup();
    }
});

test('render transaction marks only frontend source during an explicitly authorized detached handoff', async () => {
    const dom = installFakeDom();
    try {
        const { FRONTEND_SOURCE_HANDOFF_ATTRIBUTE } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/frontend-source-handoff.js'),
        );
        const { replaceMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );

        const message = document.createElement('div');
        message.classList.add('mes');
        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        Object.defineProperty(mesText, 'innerHTML', {
            configurable: true,
            get: () => '',
            set: () => {
                throw new Error('target .mes_text must not be parsed again');
            },
        });

        const frontend = '&lt;!doctype html&gt;&lt;html&gt;&lt;body&gt;card&lt;/body&gt;&lt;/html&gt;';
        const lwbFragment = '&lt;style&gt;.card{}&lt;/style&gt;&lt;div&gt;card&lt;/div&gt;';
        const lwbUrl = 'https://example.com/card.html';
        const lwbXbSrc = '&lt;!-- xb-src: https://example.com/card.html --&gt;';
        const ordinary = 'const value = &lt;div&gt;example&lt;/div&gt;;';
        replaceMesTextHtmlPreservingEmbeddedRuntimes(
            message,
            [frontend, lwbFragment, lwbUrl, lwbXbSrc, ordinary]
                .map(source => `<pre><code>${source}</code></pre>`)
                .join(''),
            { frontendSourceHandoffEvent: 'chatLoaded' },
        );

        assert.equal(message.isConnected, false);
        const pres = mesText.querySelectorAll('pre');
        assert.equal(pres.length, 5);
        assert.equal(pres[0].getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), 'chatLoaded');
        assert.equal(pres[0].textContent, '<!doctype html><html><body>card</body></html>');
        assert.equal(pres[1].getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), 'chatLoaded');
        assert.equal(pres[2].getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), 'chatLoaded');
        assert.equal(pres[3].getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), 'chatLoaded');
        assert.equal(pres[4].getAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE), null);
        assert.equal(pres[4].textContent, 'const value = <div>example</div>;');
    } finally {
        dom.cleanup();
    }
});

test('frontend source handoff rejects unsupported events and live messages', async () => {
    const dom = installFakeDom();
    try {
        const { replaceMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );

        const message = document.createElement('div');
        message.classList.add('mes');
        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        const html = '<pre><code>&lt;html&gt;card&lt;/html&gt;</code></pre>';
        assert.throws(
            () => replaceMesTextHtmlPreservingEmbeddedRuntimes(
                message,
                html,
                { frontendSourceHandoffEvent: 'message_updated' },
            ),
            /Unsupported frontend source handoff event/,
        );

        document.body.append(message);
        assert.throws(
            () => replaceMesTextHtmlPreservingEmbeddedRuntimes(
                message,
                html,
                { frontendSourceHandoffEvent: 'chatLoaded' },
            ),
            /requires a detached message/,
        );
    } finally {
        dom.cleanup();
    }
});

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

test('prepared legacy transaction cannot resurrect a wrapper removed by participant cleanup', async () => {
    const dom = installFakeDom();
    try {
        const { prepareMesTextHtmlPreservingEmbeddedRuntimes } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/message-render-transaction.js'),
        );
        const message = document.createElement('div');
        message.classList.add('mes');
        document.body.append(message);
        const mesText = document.createElement('div');
        mesText.classList.add('mes_text');
        message.append(mesText);

        const frontend = '<html><body>managed</body></html>';
        const source = createFrontendPre(frontend);
        const wrapper = document.createElement('div');
        wrapper.classList.add('TH-render');
        wrapper.append(document.createElement('iframe'), source);
        mesText.append(wrapper);

        const transaction = prepareMesTextHtmlPreservingEmbeddedRuntimes(
            message,
            `<pre><code>${frontend.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`,
        );
        wrapper.replaceWith(source);
        transaction.commit();

        assert.equal(mesText.querySelector('.TH-render'), null);
        assert.equal(wrapper.isConnected, false);
        assert.equal(mesText.querySelector('pre')?.textContent, frontend);
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

test('render transaction commits the parsed replacement when frontend blocks change', async () => {
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

        const wrapper = document.createElement('div');
        wrapper.classList.add('TH-render');
        wrapper.append(document.createElement('iframe'));
        wrapper.append(createFrontendPre('<html><body>before</body></html>'));
        mesText.append(wrapper);

        replaceMesTextHtmlPreservingEmbeddedRuntimes(message, '<pre><code>&lt;html&gt;after&lt;/html&gt;</code></pre>');

        assert.equal(mesText.querySelector('.TH-render'), null);
        assert.equal(wrapper.isConnected, false);
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
