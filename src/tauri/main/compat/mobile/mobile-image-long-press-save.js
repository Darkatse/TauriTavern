import { downloadBlobWithRuntime } from '../../../../scripts/file-export.js';
import { showExportFailureToast, showExportSuccessToast } from '../../../../scripts/download-feedback.js';
import { isAndroidRuntime } from '../../../../scripts/util/mobile-runtime.js';
import { invoke } from '../../../../tauri-bridge.js';
import { normalizeBinaryPayload } from '../../binary-utils.js';

// Android WebView does not implement the desktop image context menu, so images have no
// user-facing "save" affordance there. This compat layer adds the single action with no other
// entry point: long-press an image to save it. Desktop and iOS already provide their own native
// image menus, which is why the gesture is Android-only.
//
// Long-press detection is deliberately a second implementation next to `scripts/utils.js`'s
// `addLongPressEvent`: that helper is document-wide and imports the main script module, while this
// one installs per document (same-origin frames are patched separately) and reads targets across
// realms. Converging both into a dependency-free `util/long-press.js` is the intended follow-up; it
// is out of scope here because it would change an upstream file without changing behavior.
const DOCUMENT_INSTALLED_KEY = '__TAURITAVERN_MOBILE_IMAGE_LONG_PRESS_SAVE__';
const REMOTE_IMAGE_COMMAND = 'download_remote_image';
// Judgement calls, not measurements: long enough that a hold reads as intentional, short enough to
// still feel immediate. They should be tuned against real device feedback.
const LONG_PRESS_DELAY_MS = 500;
const MOVE_TOLERANCE_PX = 10;
const FALLBACK_FILE_NAME = 'image.png';
const SAFE_FILE_NAME_PATTERN = /^[^\\/:*?"<>|\s]{1,80}\.[A-Za-z0-9]{2,5}$/;

// Only MIME subtypes that are not already usable as a file extension need an alias.
const MIME_SUBTYPE_ALIASES = Object.freeze({
    jpeg: 'jpg',
    'svg+xml': 'svg',
    'x-icon': 'ico',
});

/**
 * @param {unknown} error Failure to describe.
 * @returns {string} Message suitable for a combined error, never empty.
 */
function errorText(error) {
    return String(error?.message || error);
}

/**
 * Resolves the image under a touch target, or null when the touch did not land on an image.
 * @param {EventTarget} eventTarget Touch event target.
 * @returns {{ imageElement: Element, source: string } | null} Matched image, if any.
 */
function resolveImageTarget(eventTarget) {
    // Same-origin frame events are observed from the parent realm, so the target element may
    // belong to another realm: duck-type instead of relying on `instanceof`.
    if (!eventTarget || eventTarget.nodeType !== 1 || typeof eventTarget.closest !== 'function') {
        return null;
    }

    const imageElement = eventTarget.closest('img');
    if (!imageElement) {
        return null;
    }

    // `currentSrc` reflects the srcset entry the browser actually decoded. Both it and `src`
    // are absolute, so the source stays fetchable from the observing realm's base URL.
    const source = String(imageElement.currentSrc || imageElement.src || '').trim();
    return source ? { imageElement, source } : null;
}

/**
 * @param {Blob} blob Image payload.
 * @returns {string} File extension without the leading dot.
 */
function resolveFileExtension(blob) {
    const subtype = String(blob?.type || '').split(';', 1)[0].trim().toLowerCase().split('/')[1] || '';
    const extension = MIME_SUBTYPE_ALIASES[subtype] || subtype;
    return /^[a-z0-9]{1,5}$/.test(extension) ? extension : 'png';
}

/**
 * @param {string} source Absolute image source URL.
 * @param {Blob} blob Image payload.
 * @returns {string} File name; `file-export` performs the final sanitization.
 */
function resolveFileName(source, blob) {
    try {
        const basename = decodeURIComponent(new URL(source).pathname.split('/').pop() || '').trim();
        if (SAFE_FILE_NAME_PATTERN.test(basename)) {
            return basename;
        }
    } catch {
        // `blob:` and inline payloads have no usable path; fall through to a generated name.
    }

    return `image-${Date.now()}.${resolveFileExtension(blob)}`;
}

/**
 * Reads the image bytes through the page, so cookies and routed resources keep working.
 * @param {string} source Absolute image source URL.
 * @returns {Promise<Blob>} Image payload.
 */
async function readImageBlob(source) {
    let response;
    try {
        response = await fetch(source);
    } catch (error) {
        // Cross-origin images without CORS headers cannot be read back by the page.
        throw new Error(`Image source is not readable: ${errorText(error)}`);
    }

    if (!response.ok) {
        throw new Error(`Failed to read image: HTTP ${response.status}`);
    }

    return response.blob();
}

/**
 * @param {string} source Absolute image source URL.
 * @returns {boolean} True for cross-origin `http(s)` sources, the only ones the host can fetch.
 */
function isCrossOriginHttpSource(source) {
    try {
        const url = new URL(source);
        return (url.protocol === 'http:' || url.protocol === 'https:')
            && url.origin !== window.location.origin;
    } catch {
        return false;
    }
}

/**
 * Reads image bytes through the host. The browser renders cross-origin images but keeps their
 * payload opaque without CORS; native HTTP has no such policy, so the host fetches instead.
 * @param {string} source Absolute image source URL.
 * @returns {Promise<{ blob: Blob, fileName: string }>} Payload plus the name the host resolved.
 */
async function readImageBlobThroughHost(source) {
    const result = await invoke(REMOTE_IMAGE_COMMAND, { url: source });
    const bytes = normalizeBinaryPayload(result?.data);
    if (!bytes.length) {
        throw new Error('Host returned an empty image payload');
    }

    const mimeType = String(result?.mimeType || '').trim() || 'application/octet-stream';
    const blob = new Blob([bytes], { type: mimeType });
    const fileName = String(result?.fileName || '').trim() || resolveFileName(source, blob);

    return { blob, fileName };
}

/**
 * Resolves the payload to save: the page first, the host as the cross-origin fallback.
 *
 * The fallback is limited to cross-origin `http(s)` sources on purpose. The host has no route for
 * the app's own virtual URLs, so proxying a same-origin failure would replace a real 404 with a
 * misleading network error.
 *
 * @param {string} source Absolute image source URL.
 * @returns {Promise<{ blob: Blob, fileName: string }>} Image payload and download name.
 */
async function readImagePayload(source) {
    try {
        const blob = await readImageBlob(source);
        return { blob, fileName: resolveFileName(source, blob) };
    } catch (pageError) {
        if (!isCrossOriginHttpSource(source)) {
            throw pageError;
        }

        try {
            return await readImageBlobThroughHost(source);
        } catch (hostError) {
            const details = `page: ${errorText(pageError)}; host: ${errorText(hostError)}`;
            throw new Error(`Image source is not readable (${details})`);
        }
    }
}

/**
 * Saves a single image through the shared export pipeline.
 * @param {string} source Absolute image source URL.
 * @returns {Promise<void>} Resolves once the export feedback has been shown.
 */
async function saveImageSource(source) {
    const { blob, fileName } = await readImagePayload(source);
    if (!blob.size) {
        throw new Error('Image payload is empty');
    }

    const result = await downloadBlobWithRuntime(blob, fileName, {
        fallbackName: FALLBACK_FILE_NAME,
    });
    showExportSuccessToast(result);
}

/**
 * Registers the long-press-to-save gesture for every `<img>` of a document.
 *
 * Installed for the main window and for every same-origin frame, because touch events do not
 * propagate out of frames. The gesture reuses the frontend image handlers' contract: the tap
 * that ends a long press is swallowed so it cannot trigger the caller's own tap action.
 *
 * @param {Window} [targetWindow] Window whose document should be watched.
 * @returns {void}
 */
export function installMobileImageLongPressSave(targetWindow = window) {
    if (!isAndroidRuntime()) {
        return;
    }

    let targetDocument;
    try {
        targetDocument = targetWindow?.document;
    } catch {
        // Cross-origin and sandboxed frames cannot expose their document.
        return;
    }

    if (!targetDocument || targetDocument[DOCUMENT_INSTALLED_KEY]) {
        return;
    }
    targetDocument[DOCUMENT_INSTALLED_KEY] = true;

    /** @type {ReturnType<typeof setTimeout> | null} */
    let pressTimer = null;
    /** @type {{ x: number, y: number } | null} */
    let pressOrigin = null;
    /** @type {{ imageElement: Element, source: string } | null} */
    let pressedImage = null;
    /** @type {Element | null} */
    let suppressedImage = null;

    const cancelPress = () => {
        clearTimeout(pressTimer);
        pressTimer = null;
        pressOrigin = null;
        pressedImage = null;
    };

    const handleTouchStart = (event) => {
        // Pinch and multi-finger gestures are never long presses.
        if (event.touches?.length !== 1) {
            cancelPress();
            return;
        }

        const image = resolveImageTarget(event.target);
        cancelPress();
        if (!image) {
            return;
        }

        pressedImage = image;
        pressOrigin = { x: event.touches[0].clientX, y: event.touches[0].clientY };
        pressTimer = setTimeout(() => {
            pressTimer = null;
            pressOrigin = null;
            pressedImage = null;
            suppressedImage = image.imageElement;

            void saveImageSource(image.source).catch((error) => {
                console.error('Failed to save image from long press:', error);
                showExportFailureToast(error);
            });
        }, LONG_PRESS_DELAY_MS);
    };

    const handleTouchMove = (event) => {
        if (!pressOrigin) {
            return;
        }

        const touch = event.touches?.[0];
        if (!touch) {
            return;
        }

        if (Math.abs(touch.clientX - pressOrigin.x) > MOVE_TOLERANCE_PX
            || Math.abs(touch.clientY - pressOrigin.y) > MOVE_TOLERANCE_PX) {
            cancelPress();
        }
    };

    // The tap that ends a long press must not reach image click handlers: the chat media
    // viewer would open on top of the save the user just asked for.
    const handleClick = (event) => {
        const imageElement = suppressedImage;
        if (!imageElement) {
            return;
        }

        suppressedImage = null;
        if (imageElement.isConnected && imageElement.contains(event.target)) {
            event.preventDefault();
            event.stopImmediatePropagation();
        }
    };

    targetDocument.addEventListener('touchstart', handleTouchStart, { passive: true });
    targetDocument.addEventListener('touchmove', handleTouchMove, { passive: true });
    targetDocument.addEventListener('touchend', cancelPress);
    targetDocument.addEventListener('touchcancel', cancelPress);
    targetDocument.addEventListener('click', handleClick, true);
}
