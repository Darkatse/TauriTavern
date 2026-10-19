// @ts-check

/**
 * The bottom-left status pill. Startup owns it while the app is starting up and removes it once
 * ready; after that any feature that needs a short-lived status line can re-create it through
 * `ensureStatusPill()`.
 *
 * It lives in its own module so consumers do not have to import the startup failure dialog (and
 * its i18n / clipboard dependencies) just to show one line.
 */

export const STATUS_PILL_ID = 'tt-startup-status-overlay';

/** True while the pill is in the DOM, i.e. some owner still has it. @returns {boolean} */
export function isStatusPillPresent() {
    return document.getElementById(STATUS_PILL_ID) !== null;
}

/** Re-creates the pill when nobody owns it, or returns the live element. @returns {HTMLDivElement} */
export function ensureStatusPill() {
    const existing = document.getElementById(STATUS_PILL_ID);
    if (existing instanceof HTMLDivElement) {
        return existing;
    }

    const element = document.createElement('div');
    element.id = STATUS_PILL_ID;
    element.style.position = 'fixed';
    element.style.left = '12px';
    element.style.bottom = '12px';
    element.style.zIndex = '999999';
    element.style.maxWidth = 'min(92vw, 560px)';
    element.style.padding = '6px 10px';
    element.style.borderRadius = '10px';
    element.style.background = 'rgba(0, 0, 0, 0.55)';
    element.style.color = '#fff';
    element.style.font = '12px/1.4 system-ui, -apple-system, Segoe UI, Roboto, sans-serif';
    element.style.pointerEvents = 'none';
    element.style.whiteSpace = 'pre-wrap';
    element.textContent = '';

    document.body.appendChild(element);
    return element;
}
