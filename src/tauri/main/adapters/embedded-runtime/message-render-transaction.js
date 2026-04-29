// @ts-check

const JSR_WRAPPER_SELECTOR = '.TH-render';
const LWB_WRAPPER_SELECTOR = '.xiaobaix-iframe-wrapper';

/**
 * @param {unknown} text
 * @returns {string}
 */
function normalizeLineEndings(text) {
    return String(text ?? '').replace(/\r\n?/g, '\n');
}

/**
 * @param {string} text
 * @returns {boolean}
 */
function isFrontendCode(text) {
    const s = normalizeLineEndings(text).toLowerCase();
    return (
        s.includes('html>') ||
        s.includes('<head>') ||
        s.includes('<body') ||
        s.includes('<!doctype') ||
        s.includes('<html') ||
        s.includes('<script')
    );
}

/**
 * @param {string} str
 */
function djb2(str) {
    let h = 5381;
    for (let i = 0; i < str.length; i += 1) {
        h = ((h << 5) + h) ^ str.charCodeAt(i);
    }
    return (h >>> 0).toString(16);
}

/**
 * @param {HTMLElement} host
 */
function isPreservableRuntimeHost(host) {
    if (host.dataset.ttRuntimeSlotId) {
        return true;
    }
    return Boolean(
        host.querySelector('iframe') ||
            host.querySelector('.tt-runtime-placeholder') ||
            host.querySelector('.tt-runtime-ghost')
    );
}

/**
 * @param {HTMLElement} pre
 */
function extractPreCodeText(pre) {
    const code = pre.querySelector('code');
    const text = code instanceof HTMLElement ? code.textContent : pre.textContent;
    return normalizeLineEndings(text || '');
}

/**
 * @param {ParentNode} root
 */
function extractFrontendBlocks(root) {
    /** @type {string[]} */
    const blocks = [];
    /** @type {HTMLElement[]} */
    const pres = [];

    for (const pre of root.querySelectorAll('pre')) {
        if (!(pre instanceof HTMLElement)) {
            continue;
        }
        const text = extractPreCodeText(pre);
        if (!text.trim()) {
            continue;
        }
        if (!isFrontendCode(text)) {
            continue;
        }
        blocks.push(text);
        pres.push(pre);
    }

    return { blocks, pres };
}

/**
 * @typedef {{ kind: 'jsr' | 'lwb'; index: number; wrapper: HTMLElement; xbHash?: string }} PreservedWrapper
 */

/**
 * @param {HTMLElement} mesText
 * @param {HTMLElement[]} frontendPres
 * @param {string[]} frontendBlocks
 */
function getPreservedWrappers(mesText, frontendPres, frontendBlocks) {
    /** @type {PreservedWrapper[]} */
    const preserved = [];

    for (let index = 0; index < frontendPres.length; index += 1) {
        const pre = frontendPres[index];
        if (!pre) {
            continue;
        }

        const jsrWrapper = pre.closest(JSR_WRAPPER_SELECTOR);
        if (jsrWrapper instanceof HTMLElement && mesText.contains(jsrWrapper) && isPreservableRuntimeHost(jsrWrapper)) {
            preserved.push({ kind: 'jsr', index, wrapper: jsrWrapper });
            continue;
        }

        const prev = pre.previousElementSibling;
        if (prev instanceof HTMLElement && prev.matches(LWB_WRAPPER_SELECTOR) && isPreservableRuntimeHost(prev)) {
            const xbHash = String(pre.dataset.xbHash || '').trim() || djb2(frontendBlocks[index] || '');
            preserved.push({ kind: 'lwb', index, wrapper: prev, xbHash });
        }
    }

    const seen = new Set();
    return preserved.filter((entry) => {
        if (seen.has(entry.wrapper)) {
            return false;
        }
        seen.add(entry.wrapper);
        return true;
    });
}

/**
 * @param {HTMLElement} pre
 * @param {string} xbHash
 */
function finalizeLittleWhiteBoxPre(pre, xbHash) {
    pre.classList.remove('xb-show');
    pre.style.display = 'none';
    pre.dataset.xbFinal = 'true';
    pre.dataset.xbHash = xbHash;
}

/**
 * Replaces `.mes_text` HTML while preserving already-rendered iframe runtimes
 * (JS-Slash-Runner: `div.TH-render`, LittleWhiteBox: `.xiaobaix-iframe-wrapper`)
 * when their frontend code blocks are unchanged.
 *
 * This render transaction primitive prevents host re-render flows
 * (`.html()/.empty()+append`) from tearing down iframe runtimes.
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 */
export function replaceMesTextHtmlPreservingJsSlashRunnerRuntimes(messageElement, html) {
    replaceMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html);
}

/**
 * Replaces `.mes_text` HTML while preserving already-rendered iframe runtimes
 * (JS-Slash-Runner: `div.TH-render`, LittleWhiteBox: `.xiaobaix-iframe-wrapper`)
 * when their frontend code blocks are unchanged.
 *
 * This render transaction primitive prevents host re-render flows
 * (`.html()/.empty()+append`) from tearing down iframe runtimes.
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 */
export function replaceMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html) {
    if (!(messageElement instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlPreservingEmbeddedRuntimes: messageElement must be an HTMLElement');
    }
    const mesText = messageElement.querySelector('.mes_text');
    if (!(mesText instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlPreservingEmbeddedRuntimes: .mes_text not found');
    }

    const { blocks: existingBlocks, pres: existingPres } = extractFrontendBlocks(mesText);
    const template = document.createElement('template');
    template.innerHTML = String(html ?? '');

    const { blocks: nextBlocks } = extractFrontendBlocks(template.content);
    const preserved = getPreservedWrappers(mesText, existingPres, existingBlocks);
    if (preserved.length === 0) {
        mesText.innerHTML = String(html ?? '');
        return;
    }

    if (nextBlocks.length !== existingBlocks.length) {
        mesText.innerHTML = String(html ?? '');
        return;
    }

    for (let i = 0; i < existingBlocks.length; i += 1) {
        if (existingBlocks[i] !== nextBlocks[i]) {
            mesText.innerHTML = String(html ?? '');
            return;
        }
    }

    const stash = document.createElement('div');
    stash.className = 'tt-runtime-stash';
    stash.style.display = 'none';
    messageElement.append(stash);

    /** @type {HTMLElement[]} */
    const wrappersToPreserve = [];
    for (const entry of preserved) {
        entry.wrapper.dataset.ttRuntimeMoving = '1';
        wrappersToPreserve.push(entry.wrapper);
        stash.append(entry.wrapper);
    }

    mesText.innerHTML = String(html ?? '');

    const { pres: nextPres } = extractFrontendBlocks(mesText);
    if (nextPres.length !== existingPres.length) {
        throw new Error('replaceMesTextHtmlPreservingEmbeddedRuntimes: frontend <pre> count mismatch');
    }

    for (const entry of preserved) {
        const pre = nextPres[entry.index];
        if (!(pre instanceof HTMLElement)) {
            throw new Error('replaceMesTextHtmlPreservingEmbeddedRuntimes: missing frontend <pre>');
        }

        if (entry.kind === 'jsr') {
            pre.replaceWith(entry.wrapper);
            continue;
        }

        if (entry.kind === 'lwb') {
            pre.before(entry.wrapper);
            finalizeLittleWhiteBoxPre(pre, entry.xbHash || djb2(extractPreCodeText(pre)));
            continue;
        }

        throw new Error('replaceMesTextHtmlPreservingEmbeddedRuntimes: unknown preserved kind');
    }

    stash.remove();

    queueMicrotask(() => {
        for (const wrapper of wrappersToPreserve) {
            delete wrapper.dataset.ttRuntimeMoving;
        }
    });
}
