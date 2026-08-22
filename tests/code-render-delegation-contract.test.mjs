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

function installButtonElementAlias() {
    const previous = Object.getOwnPropertyDescriptor(globalThis, 'HTMLButtonElement');
    Object.defineProperty(globalThis, 'HTMLButtonElement', {
        value: globalThis.HTMLElement,
        configurable: true,
    });
    return () => previous
        ? Object.defineProperty(globalThis, 'HTMLButtonElement', previous)
        : delete globalThis.HTMLButtonElement;
}

function createMessageWithFrontendCode() {
    const message = document.createElement('div');
    message.classList.add('mes');
    const content = document.createElement('div');
    content.classList.add('mes_text');
    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.textContent = '<html><body>preview</body></html>';
    pre.append(code);
    content.append(pre);
    message.append(content);
    return { message, content, pre };
}



test('code preview source or target lease cleanup collapses relocation and repeated mounts plateau', async () => {
    const dom = installFakeDom();
    const cleanupButtonAlias = installButtonElementAlias();
    try {
        const { createHtmlCodePreviewParticipant } = await importFresh(
            path.join(REPO_ROOT, 'src/scripts/html-code-preview.js'),
        );
        const participant = createHtmlCodePreviewParticipant({
            decorateCodeBlocks() {},
            releaseCodeBlocks() {},
            isEnabled: () => true,
            isSuppressed: () => false,
            shouldReplaceLastMessageByDefault: () => false,
        });

        const source = createMessageWithFrontendCode();
        const target = createMessageWithFrontendCode();
        target.message.classList.add('last_mes');
        document.body.append(source.message, target.message);
        const signal = new AbortController().signal;
        const sourceCleanup = participant.didMount({ mesid: 0, element: source.message, content: source.content, signal });
        const targetCleanup = participant.didMount({ mesid: 1, element: target.message, content: target.content, signal });
        const sourceContentCleanup = participant.didCommitContent({ mesid: 0, element: source.message, content: source.content, signal });
        const targetContentCleanup = participant.didCommitContent({ mesid: 1, element: target.message, content: target.content, signal });

        const candidates = [];
        participant.prepareContent(
            { mesid: 0, content: source.content },
            { claim: (runtimeSource, activate) => candidates.push({ source: runtimeSource, activate }) },
        );
        const runtimeCleanup = candidates[0].activate({
            source: candidates[0].source,
            mesid: 0,
            element: source.message,
            content: source.content,
            signal,
        });
        const container = source.message.querySelector('.mes-code-preview');
        const toggle = container.querySelector('.mes-code-preview-toggle');
        toggle.dispatchEvent(new dom.window.Event('click', { bubbles: true, cancelable: true }));
        assert.equal(container.closest('.mes'), target.message);

        assert.equal(target.content.isConnected, true, 'target content must never be parked in a fragment');
        targetContentCleanup();
        assert.equal(container.closest('.mes'), source.message, 'target cleanup must restore the source preview');
        runtimeCleanup();
        sourceContentCleanup();
        sourceCleanup();
        targetCleanup();
        assert.equal(source.pre.parentElement, source.content);

        for (let index = 0; index < 50; index += 1) {
            const cycleCandidates = [];
            participant.prepareContent(
                { mesid: 0, content: source.content },
                { claim: (runtimeSource, activate) => cycleCandidates.push({ source: runtimeSource, activate }) },
            );
            const dispose = cycleCandidates[0].activate({
                source: cycleCandidates[0].source,
                mesid: 0,
                element: source.message,
                content: source.content,
                signal: new AbortController().signal,
            });
            dispose();
        }
        assert.equal(document.querySelectorAll('iframe').length, 0);
        assert.equal(source.pre.parentElement, source.content);
    } finally {
        cleanupButtonAlias();
        dom.cleanup();
    }
});
