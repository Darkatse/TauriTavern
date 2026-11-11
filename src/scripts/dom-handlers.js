import { t } from './i18n.js';
import { escapeHtml, throttle, uuidv4 } from './utils.js';

const COUNTERS = '.range-block-counter input[type="number"][data-for], input.neo-range-input[type="number"][data-for]';
const validationMessages = new WeakMap();

/** The same explicit label is used by the field and its surrounding UI. */
export function getControlLabel(control) {
    return control.ownerDocument.getElementById(control.getAttribute('aria-labelledby')?.split(/\s+/)[0] ?? '')
        ?? control.labels?.[0];
}

export function getControlName(control) {
    return control.getAttribute('aria-label') || getControlLabel(control)?.textContent.trim() || '';
}

export function getEditorName(control) {
    return getControlName(control) || control.getAttribute('placeholder') || t`Text editor`;
}

/** @param {Element} element */
export function ensureElementId(element) {
    return element.id ||= `tt-${uuidv4()}`;
}

/** @param {Element} control @param {'labelledby'|'describedby'|'controls'} relation @param {...Element} targets */
export function setAriaRelation(control, relation, ...targets) {
    control.setAttribute(`aria-${relation}`, targets.map(ensureElementId).join(' '));
}

/**
 * Record the focused item and return its synchronous restoration step.
 * False means the owned collection has no remaining focusable control.
 * @param {() => Iterable<HTMLElement>} getItems
 * @param {(item: HTMLElement) => string} keyOf
 * @returns {() => boolean}
 */
export function captureFocus(getItems, keyOf) {
    const items = Array.from(getItems());
    const active = document.activeElement;
    const index = items.findIndex(item => item.contains(active));
    if (index < 0) return () => true;
    const controls = item => [item, ...item.querySelectorAll('button,input,select,textarea,a[href],summary,[tabindex]')]
        .filter(control => control.tabIndex >= 0 && !control.matches(':disabled') && $(control).is(':visible'));
    const key = keyOf(items[index]);
    const controlIndex = controls(items[index]).indexOf(active);

    return () => {
        const nextItems = Array.from(getItems());
        const sameIndex = nextItems.findIndex(item => keyOf(item) === key);
        const targetIndex = sameIndex >= 0 ? sameIndex : Math.min(index, nextItems.length - 1);
        function focusAt(index) {
            const item = nextItems[index];
            if (!item) return false;
            const candidates = controls(item);
            const target = (index === sameIndex && candidates[controlIndex]) || candidates[0];
            if (!target) return false;
            target.focus();
            return true;
        }
        if (focusAt(targetIndex)) return true;
        for (let distance = 1; distance < nextItems.length; distance++) {
            if (focusAt(targetIndex - distance) || focusAt(targetIndex + distance)) return true;
        }
        return false;
    };
}

/**
 * Preserve focus across one synchronous DOM replacement. Prepare asynchronous data before calling.
 * @param {() => Iterable<HTMLElement>} getItems
 * @param {(item: HTMLElement) => string} keyOf
 * @param {() => void} write
 * @returns {boolean}
 */
export function keepFocus(getItems, keyOf, write) {
    const restore = captureFocus(getItems, keyOf);
    write();
    return restore();
}

/** @typedef {{ group: string, input: HTMLInputElement, before: number, after: number }} NumericAdjustment */

/**
 * Restore a numeric control after its final constraints have been applied.
 * The feature's input handler remains the only writer of its business value.
 * @param {HTMLInputElement} input
 * @param {number} value The API owns missing-field defaults before restoring controls.
 * @param {NumericAdjustment[]} adjustments Changes collected by this load operation.
 * @param {string} group Localized source name supplied by the feature.
 */
export function restoreNumericInput(input, value, adjustments, group) {
    if (!Number.isFinite(value)) throw new Error(`Cannot restore a non-finite value for ${input.id}`);
    const min = input.min === '' ? (input.type === 'range' ? 0 : -Infinity) : Number(input.min);
    const max = input.max === '' ? (input.type === 'range' ? 100 : Infinity) : Number(input.max);
    input.value = String(Math.min(max, Math.max(min, value)));
    $(input).trigger('input');
    if (value < min || value > max) {
        adjustments.push({ group, input, before: value, after: input.valueAsNumber });
    }
}

/** @param {NumericAdjustment[]} adjustments */
export function showNumericAdjustments(adjustments) {
    if (!adjustments.length) return;
    const groups = new Map();
    for (const { group, input, before, after } of adjustments) {
        const name = getControlName(input) || input.id;
        if (!groups.has(group)) groups.set(group, []);
        groups.get(group).push(escapeHtml(`${name}: ${before} → ${after}`));
    }
    const message = Array.from(groups, ([group, lines]) => `<strong>${escapeHtml(group)}</strong><br>${lines.join('<br>')}`).join('<br>');
    toastr.warning(message, t`Some parameters were adjusted to their allowed range.`, { escapeHtml: false });
}

/** The existing counter markup opts into a native range's value and constraints. */
function rangeFor(input) {
    if (!input.matches(COUNTERS)) return null;
    const range = input.ownerDocument.getElementById(input.dataset.for);
    return range instanceof HTMLInputElement && range.type === 'range' ? range : null;
}

function countersFor(range) {
    return [...range.ownerDocument.querySelectorAll(`input[data-for="${CSS.escape(range.id)}"]`)]
        .filter(input => input.matches(COUNTERS));
}

function syncConstraints(input, range) {
    input.min = range.min || '0';
    input.max = range.max || '100';
    input.step = range.step || '1';
}

function clearValidation(input) {
    const message = validationMessages.get(input);
    if (!message) return;
    const descriptions = (input.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(id => id && id !== message.id);
    if (descriptions.length) input.setAttribute('aria-describedby', descriptions.join(' '));
    else input.removeAttribute('aria-describedby');
    input.removeAttribute('aria-invalid');
    message.remove();
    validationMessages.delete(input);
}

function showValidation(input) {
    let message = validationMessages.get(input);
    if (!message) {
        message = document.createElement('small');
        ensureElementId(message);
        message.className = 'range-input-error';
        message.setAttribute('role', 'status');
        (input.closest('.range-block') ?? input.parentElement).append(message);
        input.setAttribute('aria-describedby', `${input.getAttribute('aria-describedby') ?? ''} ${message.id}`.trim());
        validationMessages.set(input, message);
    }
    input.setAttribute('aria-invalid', 'true');
    message.textContent = input.validationMessage || t`Invalid value`;
}

function isValid(input) {
    return Number.isFinite(input.valueAsNumber) && input.validity.valid;
}

export function initDomHandlers() {
    function syncCounter(input) {
        const range = rangeFor(input);
        if (range) syncConstraints(input, range);
        return range;
    }
    function commitCounter(input) {
        const range = syncCounter(input);
        if (!range) return;
        if (!isValid(input)) {
            showValidation(input);
            return;
        }
        clearValidation(input);
        // Keep the public business input path, including immediate-apply controls.
        $(range).val(input.value).trigger('input', { forced: true });
    }

    document.querySelectorAll(COUNTERS).forEach(syncCounter);
    $(document).on('focusin', COUNTERS, function () { syncCounter(this); });
    $(document).on('input', COUNTERS, function () {
        if (!validationMessages.has(this)) return;
        if (isValid(this)) clearValidation(this);
        else showValidation(this);
    });
    $(document).on('change', COUNTERS, function () { commitCounter(this); });
    $(document).on('keydown', COUNTERS, function (event) {
        if (event.key !== 'Enter' || event.isDefaultPrevented() || event.originalEvent?.isComposing
            || event.keyCode === 229 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
        event.preventDefault();
        commitCounter(this);
    });
    $(document).on('input', 'input[type="range"]', function () {
        for (const input of countersFor(this)) {
            syncConstraints(input, this);
            // API handlers may update the counter after this delegated handler.
            clearValidation(input);
        }
    });
    handleInputWheel();
}

/**
 * Trap mouse wheel inside of focused number inputs to prevent scrolling their containers.
 * Native stepping keeps the browser's step base, bounds and decimal arithmetic.
 * The normal change path commits paired controls, including in Firefox.
 */
function handleInputWheel() {
    const minInterval = 25; // ms

    /**
     * @param {HTMLInputElement} input The number input element
     * @param {number} deltaY The wheel deltaY value
     */
    function updateValue(input, deltaY) {
        const range = rangeFor(input);
        if (range) syncConstraints(input, range);
        if (!Number.isFinite(input.valueAsNumber) || !(Number(input.step) > 0) || deltaY === 0) return;
        if (deltaY < 0) input.stepUp();
        else input.stepDown();
        input.dispatchEvent(new Event('input', { bubbles: true }));
        input.dispatchEvent(new Event('change', { bubbles: true }));
    }

    const updateValueThrottled = throttle(updateValue, minInterval);

    document.addEventListener('wheel', (e) => {
        // Try to carefully narrow down if we even need to fire this handler
        const input = document.activeElement instanceof HTMLInputElement ? document.activeElement : null;
        if (input && input.type === 'number' && input.hasAttribute('step')) {
            const slider = rangeFor(input);

            // Stop propagation for either target
            if (e.target === input || (slider && e.target === slider)) {
                e.stopPropagation();
                e.preventDefault();

                updateValueThrottled(input, e.deltaY);
            }
        }
    }, { passive: false });
}
