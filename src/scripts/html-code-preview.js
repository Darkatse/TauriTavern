import { getChatSurfaceParticipantRegistry } from '../tauri/main/services/chat-surface/runtime.js';

const PREVIEW_CONTAINER_CLASS = 'mes-code-preview';
const PREVIEW_FRAME_WRAP_CLASS = 'mes-code-preview-frame-wrap';
const PREVIEW_FRAME_CLASS = 'mes-code-preview-frame';
const PREVIEW_TOGGLE_BUTTON_CLASS = 'mes-code-preview-toggle';
const PREVIEW_RELOCATED_CLASS = 'mes-code-preview-relocated';
const PREVIEW_ACTIVE_HOST_CLASS = 'mes-code-preview-host-active';
const PREVIEW_PLACEHOLDER_CLASS = 'mes-code-preview-placeholder';
const PREVIEW_PENDING_CLASS = 'mes-code-preview-pending';
const PREVIEW_MESSAGE_TYPE = 'tauritavern_html_code_preview_height';
const PREVIEW_HEIGHT_FALLBACK = 220;
const LAST_MESSAGE_SELECTOR = '.mes.last_mes.swipes_visible, .mes.last_mes';
const EXPAND_ICON_CLASS = 'fa-up-right-and-down-left-from-center';
const RESTORE_ICON_CLASS = 'fa-down-left-and-up-right-to-center';

const HTML_ROOT_PATTERN = /<\s*html[\s>]/i;
const DOCTYPE_PATTERN = /<!doctype\b/i;
const SCRIPT_PATTERN = /<\s*script\b/i;
let previewCounter = 0;
let isPreviewMessageListenerBound = false;
/** @type {Map<string, HTMLIFrameElement>} */
const previewFrames = new Map();
/** @type {WeakMap<HTMLElement, PreviewExpansionState>} */
const previewExpansionStates = new WeakMap();
/** @type {HTMLElement | null} */
let activeExpandedPreview = null;

/**
 * @typedef {object} PreviewExpansionState
 * @property {boolean} expanded
 * @property {HTMLButtonElement | null} toggleButton
 * @property {HTMLElement | null} sourceMessageText
 * @property {string} sourceMessageMinHeight
 * @property {HTMLElement | null} sourcePlaceholder
 * @property {HTMLElement | null} targetMessageText
 */

/**
 * Returns true if the snippet should be rendered as an interactive frontend preview.
 * @param {string} sourceCode
 * @returns {boolean}
 */
function isInteractiveHtmlSnippet(sourceCode) {
    if (!sourceCode || typeof sourceCode !== 'string') {
        return false;
    }

    return HTML_ROOT_PATTERN.test(sourceCode)
        || DOCTYPE_PATTERN.test(sourceCode)
        || SCRIPT_PATTERN.test(sourceCode);
}

/**
 * Builds srcdoc content for an iframe preview.
 * @param {string} sourceCode
 * @returns {string}
 */
function buildPreviewSource(sourceCode) {
    const source = sourceCode.trim();
    if (!source) {
        return '';
    }

    // If it already looks like a complete document, render it as-is.
    if (DOCTYPE_PATTERN.test(source) || HTML_ROOT_PATTERN.test(source)) {
        return source;
    }

    // Standalone <script> blocks are wrapped in a minimal HTML shell.
    return [
        '<!DOCTYPE html>',
        '<html>',
        '<head>',
        '<meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width, initial-scale=1.0">',
        '</head>',
        '<body>',
        source,
        '</body>',
        '</html>',
    ].join('\n');
}

/**
 * Creates a unique preview ID.
 * @returns {string}
 */
function createPreviewId() {
    previewCounter += 1;
    return `tt-code-preview-${Date.now()}-${previewCounter}`;
}

/**
 * Creates a script block that reports iframe content height to the parent.
 * @param {string} previewId
 * @returns {string}
 */
function createHeightReporter(previewId) {
    const encodedPreviewId = JSON.stringify(previewId);
    return [
        '<script>',
        '(function(){',
        `const MESSAGE_TYPE = "${PREVIEW_MESSAGE_TYPE}";`,
        `const PREVIEW_ID = ${encodedPreviewId};`,
        'function getHeight(){',
        'const root=document.documentElement;',
        'const body=document.body;',
        'return Math.max(',
        'root?root.scrollHeight:0,',
        'root?root.offsetHeight:0,',
        'body?body.scrollHeight:0,',
        'body?body.offsetHeight:0,',
        'body?body.clientHeight:0',
        ');',
        '}',
        'function postHeight(){',
        'try{ parent.postMessage({ type: MESSAGE_TYPE, previewId: PREVIEW_ID, height: getHeight() }, "*"); }catch{}',
        '}',
        'const schedule=()=>requestAnimationFrame(postHeight);',
        'if(typeof ResizeObserver==="function"){',
        'const ro=new ResizeObserver(schedule);',
        'if(document.documentElement) ro.observe(document.documentElement);',
        'if(document.body) ro.observe(document.body);',
        '}',
        'if(typeof MutationObserver==="function"){',
        'const mo=new MutationObserver(schedule);',
        'mo.observe(document.documentElement||document,{subtree:true,childList:true,attributes:true,characterData:true});',
        '}',
        'window.addEventListener("load",()=>{postHeight();setTimeout(postHeight,50);setTimeout(postHeight,250);setTimeout(postHeight,1000);});',
        'window.addEventListener("resize",postHeight);',
        'postHeight();',
        '})();',
        '</script>',
    ].join('');
}

/**
 * Injects the height reporter script into srcdoc HTML.
 * @param {string} srcdoc
 * @param {string} previewId
 * @returns {string}
 */
function injectHeightReporter(srcdoc, previewId) {
    const reporter = createHeightReporter(previewId);
    if (/<\/body\s*>/i.test(srcdoc)) {
        return srcdoc.replace(/<\/body\s*>/i, `${reporter}</body>`);
    }
    return `${srcdoc}\n${reporter}`;
}

/**
 * Binds a single global message listener for iframe resize events.
 * @returns {void}
 */
function bindPreviewMessageListener() {
    if (isPreviewMessageListenerBound) {
        return;
    }

    isPreviewMessageListenerBound = true;
    window.addEventListener('message', (event) => {
        const data = event.data;
        if (!data || data.type !== PREVIEW_MESSAGE_TYPE || typeof data.previewId !== 'string') {
            return;
        }

        const iframe = previewFrames.get(data.previewId);
        if (!iframe) {
            return;
        }
        if (event.source !== iframe.contentWindow) {
            return;
        }

        if (!iframe.isConnected) {
            previewFrames.delete(data.previewId);
            return;
        }

        const height = Number(data.height);
        if (!Number.isFinite(height)) {
            return;
        }

        const nextHeight = Math.max(PREVIEW_HEIGHT_FALLBACK, Math.ceil(height));
        iframe.style.height = `${nextHeight}px`;
        const frameWrap = iframe.parentElement;
        if (frameWrap instanceof HTMLElement) {
            frameWrap.style.height = `${nextHeight}px`;
        }
    });
}

/**
 * Finds the text container of the currently last message.
 * @returns {HTMLElement | null}
 */
function findLastMessageTextContainer() {
    const hostMessage = document.querySelector(LAST_MESSAGE_SELECTOR);
    if (!(hostMessage instanceof HTMLElement)) {
        return null;
    }

    const messageText = hostMessage.querySelector('.mes_text');
    return messageText instanceof HTMLElement ? messageText : null;
}

/**
 * Updates the button icon and tooltip for the current expansion state.
 * @param {PreviewExpansionState | undefined} state
 * @param {boolean} expanded
 * @returns {void}
 */
function updateToggleButtonState(state, expanded) {
    const button = state?.toggleButton;
    if (!(button instanceof HTMLButtonElement)) {
        return;
    }

    button.classList.toggle('active', expanded);
    button.title = expanded
        ? 'Restore preview to original message'
        : 'Replace last message with this preview';
    button.innerHTML = `<i class="fa-solid ${expanded ? RESTORE_ICON_CLASS : EXPAND_ICON_CLASS}"></i>`;
}

/**
 * Ensures expansion state exists for the container.
 * @param {HTMLElement} container
 * @returns {PreviewExpansionState}
 */
function ensurePreviewExpansionState(container) {
    let state = previewExpansionStates.get(container);
    if (state) {
        return state;
    }

    state = {
        expanded: false,
        toggleButton: null,
        sourceMessageText: null,
        sourceMessageMinHeight: '',
        sourcePlaceholder: null,
        targetMessageText: null,
    };

    previewExpansionStates.set(container, state);
    return state;
}

/**
 * Expands a preview to replace the current last message block.
 * @param {HTMLElement} container
 * @returns {boolean}
 */
function expandPreviewToLastMessage(container) {
    const state = ensurePreviewExpansionState(container);
    if (state.expanded) {
        return true;
    }

    const sourceMessageText = container.closest('.mes_text');
    if (!(sourceMessageText instanceof HTMLElement)) {
        return false;
    }

    const targetMessageText = findLastMessageTextContainer();
    if (!(targetMessageText instanceof HTMLElement)) {
        return false;
    }

    state.sourceMessageText = sourceMessageText;
    state.sourceMessageMinHeight = sourceMessageText.style.minHeight || '';

    const sourceParent = container.parentElement;
    const targetMessageBlock = targetMessageText.parentElement;
    if (!(sourceParent instanceof HTMLElement) || !(targetMessageBlock instanceof HTMLElement)) {
        return false;
    }

    const placeholder = document.createElement('div');
    placeholder.className = PREVIEW_PLACEHOLDER_CLASS;
    placeholder.hidden = true;
    sourceParent.insertBefore(placeholder, container);

    state.sourcePlaceholder = placeholder;
    state.targetMessageText = targetMessageText;

    sourceMessageText.style.minHeight = '';
    const targetSiblings = [...targetMessageBlock.childNodes];
    targetMessageBlock.insertBefore(container, targetSiblings[targetSiblings.indexOf(targetMessageText) + 1] ?? null);

    const hostMessage = targetMessageText.closest('.mes');
    if (hostMessage instanceof HTMLElement) {
        hostMessage.classList.add(PREVIEW_ACTIVE_HOST_CLASS);
    }

    container.classList.add(PREVIEW_RELOCATED_CLASS);
    state.expanded = true;
    updateToggleButtonState(state, true);
    return true;
}

/**
 * Restores an expanded preview to its original message location.
 * @param {HTMLElement} container
 * @returns {void}
 */
function collapseExpandedPreview(container) {
    const state = previewExpansionStates.get(container);
    if (!state || !state.expanded) {
        return;
    }

    const hostMessage = state.targetMessageText?.closest('.mes');
    if (hostMessage instanceof HTMLElement) {
        hostMessage.classList.remove(PREVIEW_ACTIVE_HOST_CLASS);
    }

    if (state.sourcePlaceholder?.parentNode) {
        state.sourcePlaceholder.parentNode.insertBefore(container, state.sourcePlaceholder);
        state.sourcePlaceholder.remove();
    } else if (state.sourceMessageText instanceof HTMLElement) {
        state.sourceMessageText.append(container);
    }

    if (state.sourceMessageText instanceof HTMLElement) {
        state.sourceMessageText.style.minHeight = state.sourceMessageMinHeight;
    }

    container.classList.remove(PREVIEW_RELOCATED_CLASS);
    state.expanded = false;
    state.sourceMessageText = null;
    state.sourceMessageMinHeight = '';
    state.sourcePlaceholder = null;
    state.targetMessageText = null;

    if (activeExpandedPreview === container) {
        activeExpandedPreview = null;
    }

    updateToggleButtonState(state, false);
}

/**
 * Toggles replacement mode for a preview container.
 * @param {HTMLElement} container
 * @returns {void}
 */
function togglePreviewReplacement(container) {
    const state = ensurePreviewExpansionState(container);
    if (state.expanded) {
        collapseExpandedPreview(container);
        return;
    }

    if (activeExpandedPreview && activeExpandedPreview !== container) {
        collapseExpandedPreview(activeExpandedPreview);
    }

    if (expandPreviewToLastMessage(container)) {
        activeExpandedPreview = container;
    }
}

/**
 * Creates a toggle button to switch preview replacement mode.
 * @param {HTMLElement} container
 * @returns {HTMLButtonElement}
 */
function createPreviewToggleButton(container) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = PREVIEW_TOGGLE_BUTTON_CLASS;

    const state = ensurePreviewExpansionState(container);
    state.toggleButton = button;
    updateToggleButtonState(state, false);

    button.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        togglePreviewReplacement(container);
    });

    return button;
}

/**
 * Applies default replacement behavior if enabled.
 * @param {HTMLElement} container
 * @returns {() => void}
 */
function scheduleDefaultReplacement(container, shouldReplaceLastMessageByDefault) {
    if (!shouldReplaceLastMessageByDefault()) {
        return () => {};
    }

    const frameId = requestAnimationFrame(() => {
        if (!shouldReplaceLastMessageByDefault() || !container.isConnected) {
            return;
        }

        const hostMessage = container.closest('.mes');
        if (!(hostMessage instanceof HTMLElement) || !hostMessage.classList.contains('last_mes')) {
            return;
        }

        const state = previewExpansionStates.get(container);
        if (state?.expanded) {
            return;
        }

        togglePreviewReplacement(container);
    });
    return () => cancelAnimationFrame(frameId);
}

/**
 * Creates a sandboxed iframe node for rendering user-provided code.
 * @param {string} srcdoc
 * @param {string} previewId
 * @returns {HTMLIFrameElement}
 */
function createPreviewIframe(srcdoc, previewId) {
    const iframe = document.createElement('iframe');
    iframe.className = PREVIEW_FRAME_CLASS;
    iframe.loading = 'lazy';
    iframe.referrerPolicy = 'no-referrer';
    iframe.title = 'Interactive code preview';
    iframe.allowFullscreen = true;
    iframe.setAttribute('allowfullscreen', '');
    iframe.setAttribute('allow', 'fullscreen');
    iframe.setAttribute('sandbox', 'allow-scripts allow-forms allow-modals');
    iframe.srcdoc = injectHeightReporter(srcdoc, previewId);
    iframe.style.height = `${PREVIEW_HEIGHT_FALLBACK}px`;
    return iframe;
}

/**
 * Creates an interactive preview container for a code block.
 * @param {string} sourceCode
 * @returns {{ container: HTMLDivElement; iframe: HTMLIFrameElement; previewId: string }}
 */
function createPreviewContainer(sourceCode) {
    const previewSource = buildPreviewSource(sourceCode);
    const previewId = createPreviewId();

    const container = document.createElement('div');
    container.className = PREVIEW_CONTAINER_CLASS;
    container.addEventListener('click', (event) => event.stopPropagation());

    const frameWrap = document.createElement('div');
    frameWrap.className = PREVIEW_FRAME_WRAP_CLASS;
    frameWrap.style.height = `${PREVIEW_HEIGHT_FALLBACK}px`;

    /** @type {HTMLIFrameElement | null} */
    let iframe = null;
    try {
        iframe = createPreviewIframe(previewSource, previewId);
        frameWrap.append(iframe);
        const toggleButton = createPreviewToggleButton(container);
        container.append(frameWrap, toggleButton);
        return { container, iframe, previewId };
    } catch (error) {
        if (iframe) {
            iframe.srcdoc = '';
            iframe.removeAttribute('srcdoc');
        }
        previewExpansionStates.delete(container);
        throw error;
    }
}

/** @param {HTMLElement} preBlock @param {() => boolean} shouldReplaceLastMessageByDefault */
function activatePreview(preBlock, shouldReplaceLastMessageByDefault) {
    const codeBlock = preBlock.querySelector('code');
    const sourceCode = codeBlock?.textContent ?? '';
    const { container, iframe, previewId } = createPreviewContainer(sourceCode);
    const sourceWasHidden = preBlock.hidden === true;
    let cancelDefaultReplacement = () => {};
    try {
        preBlock.before(container);
        preBlock.hidden = true;
        previewFrames.set(previewId, iframe);
        cancelDefaultReplacement = scheduleDefaultReplacement(container, shouldReplaceLastMessageByDefault);
    } catch (error) {
        cancelDefaultReplacement();
        previewFrames.delete(previewId);
        iframe.srcdoc = '';
        iframe.removeAttribute('srcdoc');
        container.remove();
        preBlock.hidden = sourceWasHidden;
        throw error;
    }
    let disposed = false;

    return () => {
        if (disposed) {
            return;
        }
        disposed = true;
        cancelDefaultReplacement();
        collapseExpandedPreview(container);
        previewFrames.delete(previewId);
        iframe.srcdoc = '';
        iframe.removeAttribute('srcdoc');
        container.remove();
        preBlock.hidden = sourceWasHidden;
        previewExpansionStates.delete(container);
    };
}

/** @param {HTMLElement} messageElement */
function releaseRelocatedPreviewForMessage(messageElement) {
    const container = activeExpandedPreview;
    if (!(container instanceof HTMLElement)) {
        return;
    }
    const state = previewExpansionStates.get(container);
    const sourceMessage = state?.sourceMessageText?.closest('.mes');
    const targetMessage = state?.targetMessageText?.closest('.mes');
    if (sourceMessage === messageElement || targetMessage === messageElement || container.closest('.mes') === messageElement) {
        collapseExpandedPreview(container);
    }
}

/**
 * First-party ChatSurface participant. Runtime claims remain inert until the
 * host grants activation after the message is connected.
 *
 * @param {{
 *   decorateCodeBlocks: (element: HTMLElement) => void;
 *   releaseCodeBlocks: (element: HTMLElement) => void;
 *   isEnabled: () => boolean;
 *   isSuppressed: () => boolean;
 *   shouldReplaceLastMessageByDefault: () => boolean;
 * }} deps
 */
export function createHtmlCodePreviewParticipant({
    decorateCodeBlocks,
    releaseCodeBlocks,
    isEnabled,
    isSuppressed,
    shouldReplaceLastMessageByDefault,
}) {
    if ([decorateCodeBlocks, releaseCodeBlocks, isEnabled, isSuppressed, shouldReplaceLastMessageByDefault]
        .some(callback => typeof callback !== 'function')) {
        throw new TypeError('HTML code preview participant requires code decoration callbacks');
    }

    const didCommitContent = ({ element }) => {
        bindPreviewMessageListener();
        decorateCodeBlocks(element);
        return () => {
            releaseRelocatedPreviewForMessage(element);
            releaseCodeBlocks(element);
        };
    };

    return Object.freeze({
        id: 'tauritavern/html-code-preview',
        protocolVersion: 1,
        prepareContent({ content }, claims) {
            if (!isEnabled() || isSuppressed()) {
                return;
            }
            for (const codeBlock of content.querySelectorAll('pre code')) {
                const preBlock = codeBlock.parentElement;
                if (!(preBlock instanceof HTMLElement) || !preBlock.matches('pre')) {
                    continue;
                }
                if (!isInteractiveHtmlSnippet(codeBlock.textContent ?? '')) {
                    continue;
                }
                const sourceWasHidden = preBlock.hidden === true;
                const pending = document.createElement('div');
                pending.className = PREVIEW_PENDING_CLASS;
                pending.textContent = 'Interactive preview paused';
                preBlock.before(pending);
                preBlock.hidden = true;
                claims.claim(preBlock, ({ source, element }) => {
                    pending.remove();
                    releaseCodeBlocks(/** @type {HTMLElement} */ (source));
                    let disposePreview;
                    try {
                        disposePreview = activatePreview(
                            /** @type {HTMLElement} */ (source),
                            shouldReplaceLastMessageByDefault,
                        );
                    } catch (error) {
                        preBlock.hidden = sourceWasHidden;
                        decorateCodeBlocks(element);
                        throw error;
                    }
                    return () => {
                        disposePreview();
                        if (element.isConnected) {
                            preBlock.hidden = true;
                            preBlock.before(pending);
                            decorateCodeBlocks(element);
                        }
                    };
                });
            }
        },
        didMount({ element }) {
            return () => releaseRelocatedPreviewForMessage(element);
        },
        didCommitContent,
    });
}

/** @param {Parameters<typeof createHtmlCodePreviewParticipant>[0]} deps */
export function registerHtmlCodePreviewParticipant(deps) {
    return getChatSurfaceParticipantRegistry().register(createHtmlCodePreviewParticipant(deps));
}
