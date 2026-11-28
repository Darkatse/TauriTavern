import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createHtmlCodePreviewParticipant } = await importFrontend('scripts/html-code-preview.js');

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

function activatePreview(participant, { message, content }, signal) {
    const candidates = [];
    participant.prepareContent(
        { mesid: 0, content },
        { claim: (runtimeSource, activate) => candidates.push({ source: runtimeSource, activate }) },
    );
    return candidates[0].activate({ source: candidates[0].source, mesid: 0, element: message, content, signal });
}

test('code preview source or target lease cleanup collapses relocation and repeated mounts plateau', () => {
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

    const runtimeCleanup = activatePreview(participant, source, signal);
    const container = source.message.querySelector('.mes-code-preview');
    const toggle = container.querySelector('.mes-code-preview-toggle');
    toggle.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));
    expect(container.closest('.mes')).toBe(target.message);

    expect(target.content.isConnected, 'target content must never be parked in a fragment').toBe(true);
    targetContentCleanup();
    expect(container.closest('.mes'), 'target cleanup must restore the source preview').toBe(source.message);
    runtimeCleanup();
    sourceContentCleanup();
    sourceCleanup();
    targetCleanup();
    expect(source.pre.parentElement).toBe(source.content);

    for (let index = 0; index < 50; index += 1) {
        activatePreview(participant, source, new AbortController().signal)();
    }
    expect(document.querySelectorAll('iframe')).toHaveLength(0);
    expect(source.pre.parentElement).toBe(source.content);
});
