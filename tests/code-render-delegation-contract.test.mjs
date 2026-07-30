import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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

test('HTML code preview is an inert ChatSurface claim until connected activation', async () => {
    const dom = installFakeDom();
    const cleanupButtonAlias = installButtonElementAlias();
    try {
        const { createHtmlCodePreviewParticipant } = await importFresh(
            path.join(REPO_ROOT, 'src/scripts/html-code-preview.js'),
        );

        let suppressed = true;
        const participant = createHtmlCodePreviewParticipant({
            decorateCodeBlocks() {},
            releaseCodeBlocks() {},
            isEnabled: () => true,
            isSuppressed: () => suppressed,
            shouldReplaceLastMessageByDefault: () => false,
        });
        const suppressedMessage = createMessageWithFrontendCode();
        const suppressedCandidates = [];
        participant.prepareContent(
            { mesid: 0, content: suppressedMessage.content },
            { claim: (source, activate) => suppressedCandidates.push({ source, activate }) },
        );
        assert.equal(suppressedCandidates.length, 0);
        assert.equal(suppressedMessage.pre.parentElement, suppressedMessage.content);

        const fallback = createMessageWithFrontendCode();
        fallback.content.style.minHeight = '17px';
        suppressed = false;
        const candidates = [];
        participant.prepareContent(
            { mesid: 1, content: fallback.content },
            { claim: (source, activate) => candidates.push({ source, activate }) },
        );
        assert.equal(candidates.length, 1);
        assert.equal(fallback.message.querySelector('iframe'), null, 'detached claim must create no iframe');
        assert.equal(fallback.pre.parentElement, fallback.content);
        assert.equal(fallback.pre.hidden, true, 'pending runtime source must remain inert');
        assert.ok(fallback.message.querySelector('.mes-code-preview-pending'));
        assert.equal(fallback.content.style.minHeight, '17px');

        document.body.append(fallback.message);
        const cleanup = candidates[0].activate({
            source: candidates[0].source,
            mesid: 1,
            element: fallback.message,
            content: fallback.content,
            signal: new AbortController().signal,
        });
        assert.equal(fallback.pre.isConnected, true);
        assert.equal(fallback.pre.hidden, true);
        assert.equal(fallback.message.querySelector('.mes-code-preview-pending'), null);
        assert.ok(fallback.message.querySelector('.mes-code-preview'));
        assert.ok(fallback.message.querySelector('iframe'));
        assert.equal(fallback.content.style.minHeight, '17px');

        cleanup();
        assert.equal(fallback.pre.parentElement, fallback.content);
        assert.equal(fallback.pre.hidden, true);
        assert.ok(fallback.message.querySelector('.mes-code-preview-pending'));
        assert.equal(fallback.message.querySelector('iframe'), null);
        assert.equal(fallback.content.style.minHeight, '17px');
    } finally {
        cleanupButtonAlias();
        dom.cleanup();
    }
});

test('message render path delegates code runtime through the first-party participant', async () => {
    const extensionsSource = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions.js'), 'utf8');
    assert.match(extensionsSource, /'JS-Slash-Runner'/);
    assert.match(extensionsSource, /'LittleWhiteBox'/);
    assert.match(extensionsSource, /export function isCodeRenderDelegatedToThirdPartyRenderer\(\)/);
    assert.match(extensionsSource, /participantId:\s*'js-slash-runner\/message-runtime'/);
    assert.match(extensionsSource, /participantId:\s*'littlewhitebox\/message-runtime'/);
    assert.match(extensionsSource, /export async function activateRequiredChatSurfaceExtensions\(\)/);

    const scriptSource = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    assert.match(scriptSource, /registerHtmlCodePreviewParticipant\(/);
    assert.doesNotMatch(scriptSource, /renderInteractiveHtmlCodeBlocks/);
    const updateElementStart = scriptSource.indexOf('export function updateMessageElement');
    const updateElementEnd = scriptSource.indexOf('export function getCharacterAvatar', updateElementStart);
    assert.doesNotMatch(
        scriptSource.slice(updateElementStart, updateElementEnd),
        /addCopyToCodeBlocks\(/,
        'detached message materialization must not enter highlight or preview runtimes',
    );

    const previewSource = await readFile(path.join(REPO_ROOT, 'src/scripts/html-code-preview.js'), 'utf8');
    const claimStart = previewSource.indexOf('prepareContent({ content }, claims)');
    const didMountStart = previewSource.indexOf('didMount({ element })', claimStart);
    assert.ok(claimStart >= 0 && didMountStart > claimStart);
    assert.doesNotMatch(previewSource.slice(claimStart, didMountStart), /createPreviewIframe\(/);
    assert.match(previewSource.slice(claimStart, didMountStart), /claims\.claim\(preBlock/);

    const groupChatsSource = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');
    const authorizedChatOpenCalls = `${scriptSource}\n${groupChatsSource}`
        .match(/printMessages\(\{\s*frontendSourceHandoffEvent:/g) ?? [];
    assert.equal(authorizedChatOpenCalls.length, 2, 'only character and group chat-open may authorize source handoff');
    assert.match(scriptSource, /frontendSourceHandoffEvent:\s*event_types\.CHAT_LOADED/);
    assert.match(groupChatsSource, /frontendSourceHandoffEvent:\s*event_types\.CHAT_CHANGED/);

    const rossSource = await readFile(path.join(REPO_ROOT, 'src/scripts/RossAscends-mods.js'), 'utf8');
    const rossInitStart = rossSource.indexOf('export function initRossMods()');
    assert.ok(rossInitStart >= 0);
    assert.doesNotMatch(
        rossSource.slice(rossInitStart),
        /autoloadLastChat\(/,
        'chat auto-load must not race extension discovery from UI initialization',
    );

    const discoveryIndex = scriptSource.indexOf('await extensionsDiscoveryPromise;');
    const surfaceInstallIndex = scriptSource.indexOf('installChatSurfaceRuntime(');
    const autoLoadIndex = scriptSource.indexOf('void autoloadLastChat().catch');
    const initializedIndex = scriptSource.indexOf('await eventSource.emit(event_types.APP_INITIALIZED);');
    const deferredActivationIndex = scriptSource.indexOf('await activateDeferredThirdPartyExtensions');
    assert.ok(surfaceInstallIndex >= 0 && surfaceInstallIndex < autoLoadIndex);
    const installSource = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/install.js'),
        'utf8',
    );
    assert.match(installSource, /installFrontendSourceHandoff\(root\)/);
    assert.ok(discoveryIndex >= 0 && discoveryIndex < autoLoadIndex);
    const requiredActivationIndex = scriptSource.indexOf('await activateRequiredChatSurfaceExtensions()');
    const capabilityGateIndex = scriptSource.indexOf('assertRequiredChatSurfaceParticipants(requirements)');
    assert.ok(requiredActivationIndex >= 0 && requiredActivationIndex < capabilityGateIndex);
    assert.ok(capabilityGateIndex < autoLoadIndex);
    assert.ok(autoLoadIndex < initializedIndex);
    assert.ok(initializedIndex < deferredActivationIndex);
    assert.match(scriptSource.slice(autoLoadIndex, initializedIndex), /catch[\s\S]*toastr\.error/);
    assert.doesNotMatch(scriptSource, /await autoloadLastChat|await autoloadResultPromise/);
});

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
        toggle.dispatchEvent({
            type: 'click',
            preventDefault() {},
            stopPropagation() {},
        });
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
