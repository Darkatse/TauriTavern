const PREVIEW_CONTAINER_CLASS = 'mes-code-preview';
const PREVIEW_FRAME_WRAP_CLASS = 'mes-code-preview-frame-wrap';
const PREVIEW_FRAME_CLASS = 'mes-code-preview-frame';
const PREVIEW_MESSAGE_TYPE = 'tauritavern_html_code_preview_height';
const PREVIEW_HEIGHT_FALLBACK = 220;

const HTML_ROOT_PATTERN = /<\s*html[\s>]/i;
const DOCTYPE_PATTERN = /<!doctype\b/i;
const SCRIPT_PATTERN = /<\s*script\b/i;
let htmlCodeRenderEnabled = true;
let previewCounter = 0;
let isPreviewMessageListenerBound = false;
/** @type {Map<string, HTMLIFrameElement>} */
const previewFrames = new Map();

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

    container.append(frameWrap);

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
    }
}
