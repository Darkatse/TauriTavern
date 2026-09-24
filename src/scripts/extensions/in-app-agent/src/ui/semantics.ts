import { computeAccessibleName, getRole } from 'dom-accessibility-api';

export const MAX_PREVIEW = 200;
export type ElementDescription = Record<string, string | number | boolean>;

const FORM_CONTROLS = 'input,textarea,select,button';
const SENSITIVE_CONTENT = '[data-tt-sensitive],input[type="password"]';
const NAME_FROM_CONTENT_ROLES = new Set([
    'button', 'link', 'heading', 'checkbox', 'radio', 'option', 'tab',
    'menuitem', 'menuitemradio', 'menuitemcheckbox',
]);

/** Bound escaped text, so quotes/control characters cannot expand a preview without limit. */
export function preview(text: string, limit = MAX_PREVIEW) {
    let value = '';
    let size = 0;
    for (const character of text) {
        size += JSON.stringify(character).length - 2;
        if (size > limit) {
            return { value, truncated: true };
        }
        value += character;
    }
    return { value, truncated: false };
}

export function isDisabled(element: Element): boolean {
    return element.matches(':disabled') || element.closest('[aria-disabled="true"]') !== null;
}

export function isSensitive(element: Element): boolean {
    return element.matches('input[type="password"]') || element.closest('[data-tt-sensitive]') !== null;
}

export function isCheckableInput(element: Element): element is HTMLInputElement & { type: 'checkbox' | 'radio' } {
    return element instanceof HTMLInputElement && (element.type === 'checkbox' || element.type === 'radio');
}

function hasSensitiveNameSource(element: Element): boolean {
    const sources = [element];
    const hasNativeLabels = element instanceof HTMLInputElement
        || element instanceof HTMLTextAreaElement
        || element instanceof HTMLSelectElement
        || element instanceof HTMLButtonElement;
    if (hasNativeLabels) {
        sources.push(...Array.from(element.labels ?? []));
    }

    const labelIds = (element.getAttribute('aria-labelledby') ?? '').split(/\s+/);
    for (const id of labelIds) {
        const label = element.ownerDocument.getElementById(id);
        if (label) {
            sources.push(label);
        }
    }
    return sources.some(source => isSensitive(source) || source.querySelector(SENSITIVE_CONTENT) !== null);
}

/** Styles belong to one observation/action, never to a persistent DOM mirror. */
export function createSemantics() {
    const styles = new Map<Element, CSSStyleDeclaration>();

    function getStyle(element: Element): CSSStyleDeclaration {
        let result = styles.get(element);
        if (!result) {
            result = getComputedStyle(element);
            styles.set(element, result);
        }
        return result;
    }

    function excludesSubtree(element: Element): boolean {
        return element.matches('script,style,template,noscript,head,[aria-hidden="true"]')
            || getStyle(element).display === 'none';
    }

    function isVisible(element: Element): boolean {
        return !excludesSubtree(element)
            && getStyle(element).visibility === 'visible'
            && element.getClientRects().length > 0;
    }

    function isTextVisible(node: Text): boolean {
        const parent = node.parentElement;
        if (!parent) {
            return false;
        }
        if (isVisible(parent)) {
            return true;
        }
        const parentStyle = getStyle(parent);
        if (parentStyle.display !== 'contents' || parentStyle.visibility !== 'visible') {
            return false;
        }

        // display:contents has no element box. Inspect the text's own rendered range.
        const range = document.createRange();
        range.setStart(node, 0);
        range.setEnd(node, Math.min(node.length, MAX_PREVIEW));
        return range.getClientRects().length > 0;
    }

    function isScrollable(element: Element): boolean {
        if (element.clientHeight <= 0 || element.scrollHeight <= element.clientHeight) {
            return false;
        }
        return element === document.scrollingElement || /^(auto|scroll)$/.test(getStyle(element).overflowY);
    }

    function readRole(element: Element): string | null {
        if (element === document.body) {
            return 'document';
        }
        if (element.matches('.ttia-history-area')) {
            return 'region';
        }
        if (element.matches('[data-tt-sensitive]') && !element.matches(FORM_CONTROLS)) {
            return 'group';
        }
        if (element.localName === 'iframe') {
            return 'iframe';
        }
        if (element.matches('.cm-editor') || element.shadowRoot) {
            return 'element';
        }
        if (element.localName === 'summary') {
            return 'button';
        }
        if (element.localName === 'details') {
            return 'group';
        }

        const computedRole = getRole(element);
        const isSemanticRole = computedRole && !['generic', 'none', 'presentation'].includes(computedRole);
        if (isSemanticRole && /^[a-z]{1,40}$/.test(computedRole)) {
            return computedRole;
        }
        if (element.matches(FORM_CONTROLS)) {
            return 'control';
        }
        return isScrollable(element) ? 'container' : null;
    }

    function readOmissionReason(element: Element): string | null {
        if (isSensitive(element) && !element.matches(FORM_CONTROLS)) {
            return 'sensitive content';
        }
        if (element.matches('.ttia-history-area')) {
            return 'assistant conversation and tool logs';
        }
        if (element.localName === 'iframe') {
            return 'iframe content; not inspected';
        }
        if (element.shadowRoot) {
            return 'shadow DOM; not inspected';
        }
        if (element.matches('.cm-editor')) {
            return 'complex editor; use its API';
        }
        return null;
    }

    function readName(element: Element): string {
        // Associated labels can contain sensitive values too; avoid deriving a name from those contents.
        if (hasSensitiveNameSource(element)) {
            return element.getAttribute('aria-label') ?? element.getAttribute('title') ?? '';
        }
        if (element instanceof HTMLOptionElement) {
            return element.label;
        }
        return computeAccessibleName(element, {
            getComputedStyle: getStyle,
            computedStyleSupportsPseudoElements: false,
        });
    }

    function describeElement(element: Element): ElementDescription {
        const result: ElementDescription = { role: readRole(element) ?? 'element' };
        const omitted = readOmissionReason(element);
        if (omitted) {
            result.omitted = omitted;
        } else {
            const name = readName(element);
            if (name) {
                result.name = name;
            }
        }

        if (isDisabled(element)) {
            result.disabled = true;
        }
        if (element.closest('[inert]')) {
            result.inert = true;
        }
        // Native form state takes precedence; ARIA also supplies states for custom controls.
        appendAriaState(element, result);
        appendControlState(element, result);
        appendDisclosureState(element, result);

        if (isScrollable(element)) {
            result.scrollable = true;
            result.scrollTop = Math.round(element.scrollTop);
            result.scrollMax = Math.max(0, element.scrollHeight - element.clientHeight);
        }
        limitDescriptionText(result);
        return result;
    }

    return { excludesSubtree, isVisible, isTextVisible, isScrollable, readRole, readOmissionReason, describeElement };
}

export type Semantics = ReturnType<typeof createSemantics>;

function appendControlState(element: Element, result: ElementDescription) {
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
        if (element.readOnly) {
            result.readOnly = true;
        }
    }

    if (element instanceof HTMLInputElement) {
        result.inputType = element.type;
        if (isCheckableInput(element)) {
            result.checked = element.checked;
            if (element.indeterminate) {
                result.indeterminate = true;
            }
        } else if (element.type !== 'file') {
            result.value = isSensitive(element) ? '[redacted]' : element.value;
        }
    } else if (element instanceof HTMLTextAreaElement) {
        result.value = isSensitive(element) ? '[redacted]' : element.value;
    } else if (element instanceof HTMLSelectElement || element instanceof HTMLOptionElement) {
        result.value = isSensitive(element) ? '[redacted]' : element.value;
        if (element instanceof HTMLOptionElement) {
            result.selected = element.selected;
        } else if (element.multiple) {
            result.multiple = true;
        }
    }
}

function appendAriaState(element: Element, result: ElementDescription) {
    for (const state of ['checked', 'selected', 'expanded', 'readonly']) {
        const value = element.getAttribute(`aria-${state}`);
        if (value !== 'true' && value !== 'false' && value !== 'mixed') {
            continue;
        }
        const key = state === 'readonly' ? 'readOnly' : state;
        result[key] = value === 'mixed' ? value : value === 'true';
    }
}

function appendDisclosureState(element: Element, result: ElementDescription) {
    if (element instanceof HTMLDetailsElement) {
        result.expanded = element.open;
    }
    if (element.localName === 'summary' && element.parentElement instanceof HTMLDetailsElement) {
        result.expanded = element.parentElement.open;
    }
    if (element instanceof HTMLDialogElement && CSS.supports('selector(:modal)')) {
        result.modal = element.matches(':modal');
    }
}

function limitDescriptionText(description: ElementDescription) {
    const textFields = Object.entries(description).filter(
        (entry): entry is [string, string] => entry[0] !== 'role' && typeof entry[1] === 'string',
    );
    for (const [key, text] of textFields) {
        const bounded = preview(text, Math.floor(MAX_PREVIEW / textFields.length));
        description[key] = bounded.value;
        if (bounded.truncated) {
            description[`${key}Truncated`] = true;
        }
    }
}

export function suppressesText(element: Element, role: string | null): boolean {
    return (role !== null && NAME_FROM_CONTENT_ROLES.has(role)) || element.matches('input,textarea,select');
}

export function describeLine(description: ElementDescription): string {
    const { role, name, ...states } = description;
    const parts = [String(role)];
    if (name) {
        parts.push(JSON.stringify(name));
    }
    for (const [key, value] of Object.entries(states)) {
        parts.push(`${key}=${JSON.stringify(value)}`);
    }
    return parts.join(' ');
}
