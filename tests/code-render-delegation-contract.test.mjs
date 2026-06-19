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

function installJqueryShim() {
    const previousDollar = Object.getOwnPropertyDescriptor(globalThis, '$');
    const previousJquery = Object.getOwnPropertyDescriptor(globalThis, 'jQuery');

    const createCollection = (elements) => ({
        length: elements.length,
        get(index) {
            return elements[index];
        },
        is(selector) {
            return Boolean(elements[0]?.matches?.(selector));
        },
        closest(selector) {
            return createCollection(elements.map(el => el?.closest?.(selector)).filter(Boolean));
        },
        find(selector) {
            const found = [];
            for (const el of elements) {
                if (!el?.querySelectorAll) {
                    continue;
                }

                if (selector === 'pre > code') {
                    found.push(...el.querySelectorAll('code').filter(code => code.parentElement?.matches('pre')));
                    continue;
                }

                found.push(...el.querySelectorAll(selector));
            }
            return createCollection(found);
        },
    });

    const shim = (input) => createCollection(input ? [input] : []);
    Object.defineProperty(globalThis, '$', { value: shim, configurable: true });
    Object.defineProperty(globalThis, 'jQuery', { value: shim, configurable: true });

    return () => {
        if (previousDollar) {
            Object.defineProperty(globalThis, '$', previousDollar);
        } else {
            delete globalThis.$;
        }

        if (previousJquery) {
            Object.defineProperty(globalThis, 'jQuery', previousJquery);
        } else {
            delete globalThis.jQuery;
        }
    };
}

function installButtonElementAlias() {
    const previous = Object.getOwnPropertyDescriptor(globalThis, 'HTMLButtonElement');
    Object.defineProperty(globalThis, 'HTMLButtonElement', {
        value: globalThis.HTMLElement,
        configurable: true,
    });

    return () => {
        if (previous) {
            Object.defineProperty(globalThis, 'HTMLButtonElement', previous);
        } else {
            delete globalThis.HTMLButtonElement;
        }
    };
}

function createMessageWithFrontendCode() {
    const message = document.createElement('div');
    message.classList.add('mes');

    const mesText = document.createElement('div');
    mesText.classList.add('mes_text');

    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.textContent = '<html><body>preview</body></html>';
    pre.append(code);
    mesText.append(pre);
    message.append(mesText);
    document.body.append(message);

    return { message, pre };
}

test('html code preview suppression preserves code blocks for delegated renderers', async () => {
    const dom = installFakeDom();
    const cleanupJquery = installJqueryShim();
    const cleanupButtonAlias = installButtonElementAlias();

    try {
        const {
            renderInteractiveHtmlCodeBlocks,
            setHtmlCodeRenderEnabled,
            setHtmlCodeRenderReplaceLastMessageByDefault,
            setHtmlCodeRenderSuppressedByExternalRenderer,
        } = await importFresh(path.join(REPO_ROOT, 'src/scripts/html-code-preview.js'));

        const suppressed = createMessageWithFrontendCode();
        setHtmlCodeRenderEnabled(true);
        setHtmlCodeRenderReplaceLastMessageByDefault(false);
        setHtmlCodeRenderSuppressedByExternalRenderer(true);
        renderInteractiveHtmlCodeBlocks(suppressed.message);

        assert.equal(suppressed.pre.isConnected, true);
        assert.equal(suppressed.message.querySelector('.mes-code-preview'), null);

        const fallback = createMessageWithFrontendCode();
        setHtmlCodeRenderSuppressedByExternalRenderer(false);
        renderInteractiveHtmlCodeBlocks(fallback.message);

        assert.equal(fallback.pre.isConnected, false);
        assert.ok(fallback.message.querySelector('.mes-code-preview'));
    } finally {
        cleanupButtonAlias();
        cleanupJquery();
        dom.cleanup();
    }
});

test('message render path delegates code-render to known third-party renderers', async () => {
    const extensionsSource = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions.js'), 'utf8');
    assert.match(extensionsSource, /'JS-Slash-Runner'/);
    assert.match(extensionsSource, /'LittleWhiteBox'/);
    assert.match(extensionsSource, /export function isCodeRenderDelegatedToThirdPartyRenderer\(\)/);
    assert.match(extensionsSource, /findExtension\(name\)\?\.enabled === true/);

    const scriptSource = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const start = scriptSource.indexOf('export function addCopyToCodeBlocks');
    assert.ok(start >= 0, 'addCopyToCodeBlocks must exist');
    const end = scriptSource.indexOf('const coordinator = getCodeHighlightCoordinator();', start);
    assert.ok(end > start, 'addCopyToCodeBlocks setup section must contain code highlight coordinator');
    const section = scriptSource.slice(start, end);

    const suppressionIndex = section.indexOf('setHtmlCodeRenderSuppressedByExternalRenderer(');
    const delegationIndex = section.indexOf('isCodeRenderDelegatedToThirdPartyRenderer()', suppressionIndex);
    const renderIndex = section.indexOf('renderInteractiveHtmlCodeBlocks(messageElement);');
    assert.ok(suppressionIndex >= 0, 'message render path must sync code-render delegation');
    assert.ok(delegationIndex > suppressionIndex, 'delegation sync must consult known third-party renderers');
    assert.ok(renderIndex > suppressionIndex, 'delegation state must be synced before rendering previews');
});
