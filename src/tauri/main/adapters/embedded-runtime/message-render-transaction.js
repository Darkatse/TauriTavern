// @ts-check

import {
    assertFrontendSourceHandoffEvent,
    markFrontendSourceHandoff,
} from '../chat-surface/frontend-source-handoff.js';
import { syncElementAttributes } from '../../../../scripts/tauri/message/mes-text-content.js';

const JSR_WRAPPER_SELECTOR = '.TH-render';
const LWB_WRAPPER_SELECTOR = '.xiaobaix-iframe-wrapper';

// Mirrors LittleWhiteBox's code-block acceptance boundary.
const LWB_EXTERNAL_URL_PATTERN = /^https?:\/\/[^\s]+$/i;
const LWB_XB_SRC_PATTERN = /<!--\s*xb-src:\s*(https?:\/\/[^\s>]+)\s*-->/i;
const LWB_HTML_FRAGMENT_START_PATTERN = /^\s*(?:<!--[\s\S]*?-->\s*)*<(?:style|link|meta|svg|iframe|canvas|img|video|audio|picture|div|section|main|article|header|footer|nav|aside|p|span|button|input|textarea|select|label|ul|ol|li|table|thead|tbody|tr|td|th|form|figure|figcaption|details|summary|dialog|h[1-6])\b/i;

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
    const source = normalizeLineEndings(text).trim();
    const lower = source.toLowerCase();
    if (
        lower.includes('html>') ||
        lower.includes('<head>') ||
        lower.includes('<body') ||
        lower.includes('<!doctype') ||
        lower.includes('<html') ||
        lower.includes('<script')
    ) {
        return true;
    }

    return LWB_EXTERNAL_URL_PATTERN.test(source) ||
        LWB_XB_SRC_PATTERN.test(source) ||
        LWB_HTML_FRAGMENT_START_PATTERN.test(source);
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
 * The render transaction prevents host re-render flows
 * (`.html()/.empty()+append`) from tearing down iframe runtimes.
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 * @param {{ frontendSourceHandoffEvent?: string | null }} [options]
 */
export function replaceMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html, { frontendSourceHandoffEvent = null } = {}) {
    prepareMesTextHtmlPreservingEmbeddedRuntimes(
        messageElement,
        html,
        { frontendSourceHandoffEvent },
    ).commit();
}

/**
 * Parses replacement content once into a detached `.mes_text`; commit moves
 * its attributes and children into the stable live wrapper.
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 * @param {{ frontendSourceHandoffEvent?: string | null }} [options]
 */
export function prepareMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html, { frontendSourceHandoffEvent = null } = {}) {
    if (!(messageElement instanceof HTMLElement)) {
        throw new Error('prepareMesTextHtmlPreservingEmbeddedRuntimes: messageElement must be an HTMLElement');
    }
    const mesText = messageElement.querySelector('.mes_text');
    if (!(mesText instanceof HTMLElement)) {
        throw new Error('prepareMesTextHtmlPreservingEmbeddedRuntimes: .mes_text not found');
    }
    const targetMesText = mesText;

    if (frontendSourceHandoffEvent !== null) {
        assertFrontendSourceHandoffEvent(frontendSourceHandoffEvent);
        if (messageElement.isConnected) {
            throw new Error('prepareMesTextHtmlPreservingEmbeddedRuntimes: frontend source handoff requires a detached message');
        }
    }

    const stagingMesText = /** @type {HTMLElement} */ (mesText.cloneNode(false));
    stagingMesText.innerHTML = String(html ?? '');

    const { pres: nextPres } = extractFrontendBlocks(stagingMesText);
    if (frontendSourceHandoffEvent !== null) {
        markFrontendSourceHandoff(nextPres, frontendSourceHandoffEvent);
    }

    let committed = false;

    function commit() {
        if (committed) {
            throw new Error('Chat message content transaction was already committed');
        }
        committed = true;

        // ChatSurface closes managed content leases before commit. Taking this
        // snapshot now preserves only legacy wrappers which still have an owner.
        const { blocks: existingBlocks, pres: existingPres } = extractFrontendBlocks(targetMesText);
        const preserved = getPreservedWrappers(targetMesText, existingPres, existingBlocks);
        const { blocks: nextBlocks, pres: committedPres } = extractFrontendBlocks(stagingMesText);
        const canPreserve = preserved.length > 0 &&
            nextBlocks.length === existingBlocks.length &&
            existingBlocks.every((block, index) => block === nextBlocks[index]);

        if (!canPreserve) {
            syncElementAttributes(targetMesText, stagingMesText);
            targetMesText.replaceChildren(...stagingMesText.childNodes);
            return targetMesText;
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

        syncElementAttributes(targetMesText, stagingMesText);
        targetMesText.replaceChildren(...stagingMesText.childNodes);

        for (const entry of preserved) {
            const pre = committedPres[entry.index];
            if (!(pre instanceof HTMLElement)) {
                throw new Error('prepareMesTextHtmlPreservingEmbeddedRuntimes: missing frontend <pre>');
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

            throw new Error('prepareMesTextHtmlPreservingEmbeddedRuntimes: unknown preserved kind');
        }

        stash.remove();
        queueMicrotask(() => {
            for (const wrapper of wrappersToPreserve) {
                delete wrapper.dataset.ttRuntimeMoving;
            }
        });
        return targetMesText;
    }

    return Object.freeze({ content: stagingMesText, commit });
}
