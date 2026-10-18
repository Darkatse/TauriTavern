// @ts-check

/**
 * Renders chat-write progress in the composer placeholder.
 *
 * `commit.js` broadcasts `tt-chat-commit` from the single place every chat write passes through
 * (character/group x payload/metadata/metadataExtension), so this module only formats it.
 *
 * The placeholder is the least invasive surface: it needs no new DOM and no layout height, which
 * matters on mobile. The trade-off is that it is only visible while the composer is empty, so a
 * chat write that happens mid-typing stays invisible.
 *
 * It deliberately does not write `#chat_generation_status`: that element is the screen-reader
 * projection of the current generation stage, and per-frame save percentages would be noise there
 * (see docs/FrontendAccessibility.md, "生成状态说明…不逐 token 公告正文").
 *
 * Labels are ASCII and short (`savechat:53%`) because a localized label does not fit this line on
 * a phone. They are derived from the commit `kind`, so the transport layer stays free of UI copy.
 */

export const SAVE_PROGRESS_EVENT = 'tt-chat-commit';

/** A single-frame metadata write finishes in milliseconds; without a floor the line only flashes. */
export const SAVE_PROGRESS_MIN_VISIBLE_MS = 400;

/** Consecutive writes drain the queue momentarily between them; bridge that gap. */
export const SAVE_PROGRESS_SETTLE_MS = 120;

const SHORT_LABELS = Object.freeze({
    payload: 'savechat',
    metadata: 'savemeta',
    metadataExtension: 'savemeta',
});

/**
 * @typedef {{
 *   getTextarea: () => HTMLTextAreaElement | null;
 *   target?: EventTarget;
 *   now?: () => number;
 *   schedule?: (callback: () => void, ms: number) => any;
 *   cancel?: (timer: any) => void;
 * }} SaveProgressIndicatorOptions
 */

/**
 * @param {SaveProgressIndicatorOptions} options
 */
export function createSaveProgressIndicator({
    getTextarea,
    target = window,
    now = () => performance.now(),
    schedule = (callback, ms) => setTimeout(callback, ms),
    cancel = timer => clearTimeout(timer),
}) {
    /** @type {HTMLTextAreaElement | null} */
    let shownOn = null;
    /** @type {string | null} */
    let baseline = null;
    /** @type {string | null} */
    let written = null;
    let active = false;
    let shownAt = 0;
    /** @type {any} */
    let settleTimer = null;

    /** @param {HTMLTextAreaElement} textarea */
    const read = textarea => textarea.getAttribute('placeholder');

    function cancelSettle() {
        if (settleTimer === null) return;
        cancel(settleTimer);
        settleTimer = null;
    }

    /** @param {string | null} value */
    function write(value) {
        if (!shownOn) return;
        written = value;
        if (value === null) {
            shownOn.removeAttribute('placeholder');
        } else {
            shownOn.setAttribute('placeholder', value);
        }
    }

    function restore() {
        cancelSettle();
        active = false;
        const textarea = shownOn;
        const previous = baseline;
        shownOn = null;
        baseline = null;
        // Only undo our own text: the host or another extension may own the placeholder by now.
        if (textarea && read(textarea) === written) {
            written = null;
            if (previous === null) {
                textarea.removeAttribute('placeholder');
            } else {
                textarea.setAttribute('placeholder', previous);
            }
            return;
        }
        written = null;
    }

    /** @param {{ kind?: unknown, percent?: unknown }} primary */
    function present(primary) {
        const textarea = getTextarea();
        if (!textarea) return;
        if (!active) {
            // Capture the host's own placeholder right before the first write.
            baseline = read(textarea);
            shownOn = textarea;
            active = true;
            shownAt = now();
        } else if (read(textarea) !== written) {
            // Someone else replaced our text: adopt theirs as the baseline to return to.
            baseline = read(textarea);
            shownOn = textarea;
        }
        const kind = typeof primary.kind === 'string' ? primary.kind : '';
        const percent = Number.isFinite(primary.percent) ? Math.round(Number(primary.percent)) : 0;
        const text = `${SHORT_LABELS[kind] ?? 'save'}:${percent}%`;
        if (read(textarea) === text) {
            written = text;
            return;
        }
        write(text);
    }

    function hide() {
        settleTimer = null;
        if (!active) return;
        const remaining = SAVE_PROGRESS_MIN_VISIBLE_MS - (now() - shownAt);
        if (remaining > 0) {
            settleTimer = schedule(hide, remaining);
            return;
        }
        restore();
    }

    /** @param {Event} event */
    function onCommit(event) {
        const detail = /** @type {CustomEvent} */ (event).detail;
        const primary = detail?.primary ?? null;
        if (primary) {
            cancelSettle();
            present(primary);
            return;
        }
        if (!active) return;
        cancelSettle();
        settleTimer = schedule(hide, SAVE_PROGRESS_SETTLE_MS);
    }

    target.addEventListener(SAVE_PROGRESS_EVENT, onCommit);

    return {
        dispose() {
            target.removeEventListener(SAVE_PROGRESS_EVENT, onCommit);
            restore();
        },
    };
}

/** The disconnected composer advertises "Not connected to API!"; do not cover that. */
function resolveComposerTextarea() {
    const textarea = document.querySelector('#send_textarea');
    if (!(textarea instanceof HTMLTextAreaElement)) return null;
    if (document.querySelector('#send_form')?.classList.contains('no-connection')) return null;
    return textarea;
}

/**
 * Install the indicator and advertise the capability so extensions that render their own save
 * progress can stand down instead of writing over it.
 */
export function installSaveProgressIndicator() {
    const indicator = createSaveProgressIndicator({ getTextarea: resolveComposerTextarea });
    document.documentElement.dataset.ttSaveProgress = 'on';
    return {
        dispose() {
            indicator.dispose();
            delete document.documentElement.dataset.ttSaveProgress;
        },
    };
}
