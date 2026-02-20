const PREVIEW_CONTAINER_CLASS = 'mes-code-preview';
const PREVIEW_FRAME_WRAP_CLASS = 'mes-code-preview-frame-wrap';
const PREVIEW_FRAME_CLASS = 'mes-code-preview-frame';
const PREVIEW_TOGGLE_BUTTON_CLASS = 'mes-code-preview-toggle';
const PREVIEW_RELOCATED_CLASS = 'mes-code-preview-relocated';
const PREVIEW_ACTIVE_HOST_CLASS = 'mes-code-preview-host-active';
const PREVIEW_PLACEHOLDER_CLASS = 'mes-code-preview-placeholder';
const PREVIEW_MESSAGE_TYPE = 'tauritavern_html_code_preview_height';
const PREVIEW_HEIGHT_FALLBACK = 220;
const LAST_MESSAGE_SELECTOR = '.mes.last_mes.swipes_visible, .mes.last_mes';
const EXPAND_ICON_CLASS = 'fa-up-right-and-down-left-from-center';
const RESTORE_ICON_CLASS = 'fa-down-left-and-up-right-to-center';

const HTML_ROOT_PATTERN = /<\s*html[\s>]/i;
const DOCTYPE_PATTERN = /<!doctype\b/i;
const SCRIPT_PATTERN = /<\s*script\b/i;
let htmlCodeRenderEnabled = false;
let replaceLastMessageByDefault = false;
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
 * @property {string} targetMessageMinHeight
 * @property {DocumentFragment | null} targetContent
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
 * Removes disconnected iframes from registry.
 * @returns {void}
 */
function cleanupPreviewFrames() {
    for (const [previewId, frame] of previewFrames.entries()) {
        if (!frame.isConnected) {
            previewFrames.delete(previewId);
        }
    }

    if (activeExpandedPreview && !activeExpandedPreview.isConnected) {
        const state = previewExpansionStates.get(activeExpandedPreview);
        if (state?.targetMessageText instanceof HTMLElement) {
            state.targetMessageText.closest('.mes')?.classList.remove(PREVIEW_ACTIVE_HOST_CLASS);
            if (state.targetContent instanceof DocumentFragment) {
                state.targetMessageText.replaceChildren();
                state.targetMessageText.append(state.targetContent);
                state.targetMessageText.style.minHeight = state.targetMessageMinHeight;
            }
        }

        activeExpandedPreview = null;
    }
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
            syncMessageTextHeight(frameWrap, nextHeight);
        }
    });
}

/**
 * Expands the host message text area so iframe previews are fully visible.
 * @param {HTMLElement} frameWrap
 * @param {number} previewHeight
 * @returns {void}
 */
function syncMessageTextHeight(frameWrap, previewHeight) {
    if (!Number.isFinite(previewHeight) || previewHeight <= 0) {
        return;
    }

    const messageText = frameWrap.closest('.mes_text');
    if (!(messageText instanceof HTMLElement)) {
        return;
    }

    const wrapRect = frameWrap.getBoundingClientRect();
    const messageRect = messageText.getBoundingClientRect();
    const requiredHeight = Math.ceil(wrapRect.bottom - messageRect.top);
    if (requiredHeight <= 0) {
        return;
    }

    const currentMinHeight = Number.parseFloat(messageText.style.minHeight);
    const nextMinHeight = Number.isFinite(currentMinHeight)
        ? Math.max(currentMinHeight, requiredHeight)
        : requiredHeight;

    messageText.style.minHeight = `${nextMinHeight}px`;
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
 * Returns the current frame height for a preview container.
 * @param {HTMLElement} container
 * @returns {number}
 */
function getPreviewHeight(container) {
    const iframe = container.querySelector(`.${PREVIEW_FRAME_CLASS}`);
    if (!(iframe instanceof HTMLIFrameElement)) {
        return PREVIEW_HEIGHT_FALLBACK;
    }

    const height = Number.parseFloat(iframe.style.height);
    return Number.isFinite(height) ? Math.max(PREVIEW_HEIGHT_FALLBACK, Math.ceil(height)) : PREVIEW_HEIGHT_FALLBACK;
}

/**
 * Keeps the current message text block sized correctly after preview moves.
 * @param {HTMLElement} container
 * @returns {void}
 */
function syncPreviewContainerHeight(container) {
    const frameWrap = container.querySelector(`.${PREVIEW_FRAME_WRAP_CLASS}`);
    if (!(frameWrap instanceof HTMLElement)) {
        return;
    }

    syncMessageTextHeight(frameWrap, getPreviewHeight(container));
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
        targetMessageMinHeight: '',
        targetContent: null,
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

    if (sourceMessageText === targetMessageText) {
        const hostMessage = sourceMessageText.closest('.mes');
        if (hostMessage instanceof HTMLElement) {
            hostMessage.classList.add(PREVIEW_ACTIVE_HOST_CLASS);
        }

        container.classList.add(PREVIEW_RELOCATED_CLASS);
        state.expanded = true;
        updateToggleButtonState(state, true);
        syncPreviewContainerHeight(container);
        return true;
    }

    const sourceParent = container.parentElement;
    if (!(sourceParent instanceof HTMLElement)) {
        return false;
    }

    const placeholder = document.createElement('div');
    placeholder.className = PREVIEW_PLACEHOLDER_CLASS;
    placeholder.hidden = true;
    sourceParent.insertBefore(placeholder, container);

    const preservedTargetContent = document.createDocumentFragment();
    while (targetMessageText.firstChild) {
        preservedTargetContent.append(targetMessageText.firstChild);
    }

    state.sourcePlaceholder = placeholder;
    state.targetMessageText = targetMessageText;
    state.targetMessageMinHeight = targetMessageText.style.minHeight || '';
    state.targetContent = preservedTargetContent;

    sourceMessageText.style.minHeight = '';
    targetMessageText.append(container);

    const hostMessage = targetMessageText.closest('.mes');
    if (hostMessage instanceof HTMLElement) {
        hostMessage.classList.add(PREVIEW_ACTIVE_HOST_CLASS);
    }

    container.classList.add(PREVIEW_RELOCATED_CLASS);
    state.expanded = true;
    updateToggleButtonState(state, true);
    syncPreviewContainerHeight(container);
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

    const hostMessage = container.closest('.mes');
    if (hostMessage instanceof HTMLElement) {
        hostMessage.classList.remove(PREVIEW_ACTIVE_HOST_CLASS);
    }

    if (state.targetMessageText instanceof HTMLElement && state.targetContent instanceof DocumentFragment) {
        state.targetMessageText.replaceChildren();
        state.targetMessageText.append(state.targetContent);
        state.targetMessageText.style.minHeight = state.targetMessageMinHeight;
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
    state.targetMessageMinHeight = '';
    state.targetContent = null;

    if (activeExpandedPreview === container) {
        activeExpandedPreview = null;
    }

    updateToggleButtonState(state, false);
    syncPreviewContainerHeight(container);
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
 * @returns {void}
 */
function scheduleDefaultReplacement(container) {
    if (!replaceLastMessageByDefault) {
        return;
    }

    requestAnimationFrame(() => {
        if (!replaceLastMessageByDefault || !container.isConnected) {
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
    iframe.setAttribute('sandbox', 'allow-scripts allow-forms allow-modals');
    iframe.srcdoc = injectHeightReporter(srcdoc, previewId);
    iframe.style.height = `${PREVIEW_HEIGHT_FALLBACK}px`;
    return iframe;
}

/**
 * Creates an interactive preview container for a code block.
 * @param {string} sourceCode
 * @returns {HTMLDivElement}
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

    const iframe = createPreviewIframe(previewSource, previewId);
    previewFrames.set(previewId, iframe);
    frameWrap.append(iframe);

    const toggleButton = createPreviewToggleButton(container);

    container.append(frameWrap);
    container.append(toggleButton);

    return container;
}

/**
 * Returns true if the current root belongs to a chat message block.
 * @param {JQuery<HTMLElement>} $root
 * @returns {boolean}
 */
function isMessageContext($root) {
    return $root.is('.mes') || $root.closest('.mes').length > 0;
}

/**
 * Enables or disables chat HTML code rendering.
 * @param {boolean} enabled
 * @returns {void}
 */
export function setHtmlCodeRenderEnabled(enabled) {
    htmlCodeRenderEnabled = !!enabled;
}

/**
 * Configures whether the newest rendered preview should replace the last message by default.
 * @param {boolean} enabled
 * @returns {void}
 */
export function setHtmlCodeRenderReplaceLastMessageByDefault(enabled) {
    replaceLastMessageByDefault = !!enabled;
}

/**
 * Adds interactive preview controls for renderable HTML/script code blocks in chat messages.
 * @param {JQuery<HTMLElement> | HTMLElement} messageElement
 * @returns {void}
 */
export function renderInteractiveHtmlCodeBlocks(messageElement) {
    if (!htmlCodeRenderEnabled) {
        return;
    }

    bindPreviewMessageListener();
    cleanupPreviewFrames();

    const $root = $(messageElement);
    if (!$root.length || !isMessageContext($root)) {
        return;
    }

    const codeBlocks = $root.find('pre > code');
    for (let i = 0; i < codeBlocks.length; i++) {
        const codeBlock = codeBlocks.get(i);
        const preBlock = codeBlock?.closest('pre');
        if (!(preBlock instanceof HTMLElement)) {
            continue;
        }

        const sourceCode = codeBlock.textContent ?? '';
        if (!isInteractiveHtmlSnippet(sourceCode)) {
            continue;
        }

        const previewContainer = createPreviewContainer(sourceCode);
        preBlock.replaceWith(previewContainer);
        const frameWrap = previewContainer.querySelector(`.${PREVIEW_FRAME_WRAP_CLASS}`);
        if (frameWrap instanceof HTMLElement) {
            syncMessageTextHeight(frameWrap, PREVIEW_HEIGHT_FALLBACK);
        }
        scheduleDefaultReplacement(previewContainer);
    }
}
