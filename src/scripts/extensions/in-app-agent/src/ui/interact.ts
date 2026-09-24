import { createSemantics, isCheckableInput, isDisabled } from './semantics';
import type { Semantics } from './semantics';
import type { Observation } from './snapshot';

type ToolArguments = Record<string, TauriTavernJsonValue>;
type Interaction =
    | { action: 'click'; ref: string }
    | { action: 'fill'; ref: string; value: string }
    | { action: 'select'; ref: string; value: string }
    | { action: 'set_checked'; ref: string; checked: boolean }
    | { action: 'scroll'; ref: string; direction: 'up' | 'down' };
type ChatScroller = (options: ScrollToOptions) => void;

const ACTION_FIELDS: Record<Interaction['action'], string[]> = {
    click: [],
    fill: ['value'],
    select: ['value'],
    set_checked: ['checked'],
    scroll: ['direction'],
};
const FILLABLE_INPUT_TYPES = new Set(['text', 'search', 'email', 'url', 'tel', 'number']);

export async function interact(args: ToolArguments, observation: Observation, signal: AbortSignal) {
    const request = parseInteraction(args);
    let scrollChat: ChatScroller | undefined;

    if (request.action === 'scroll' && observation.resolve(request.ref).id === 'chat') {
        scrollChat = await loadChatScroller();
    }

    // Loading the page module can yield. Resolve again before checking or changing the target.
    if (signal.aborted) {
        throw new Error('The operation was cancelled before acting.');
    }
    const element = requireInteractiveElement(observation.resolve(request.ref));
    const apply = prepareAction(request, element, createSemantics(), scrollChat);
    observation.clearCursor();

    // Preparation has no side effects. Once apply starts, errors must not imply nothing happened.
    try {
        const dispatched = apply();
        const observed = element.isConnected
            ? { connected: true, ...createSemantics().describeElement(element) }
            : { connected: false };
        return { action: request.action, dispatched, observed };
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        throw new Error(`The action was attempted, but its result could not be confirmed. Changes may already have taken effect. Check the current state with app.snapshot or a known API before retrying. Details: ${message}`);
    }
}

function parseInteraction(args: ToolArguments): Interaction {
    if (typeof args.action !== 'string' || !Object.hasOwn(ACTION_FIELDS, args.action)) {
        throw new Error('Choose click, fill, select, set_checked or scroll.');
    }
    if (typeof args.ref !== 'string') {
        throw new Error('ref must be a string copied from the latest app.snapshot page.');
    }

    const action = args.action as Interaction['action'];
    const ref = args.ref;
    const allowedFields = ['action', 'ref', ...ACTION_FIELDS[action]];
    const unexpectedFields = Object.keys(args).filter(key => !allowedFields.includes(key));
    if (unexpectedFields.length > 0) {
        throw new Error(`${action} does not accept ${unexpectedFields.join(', ')}. Pass only ${allowedFields.join(', ')}.`);
    }

    switch (action) {
        case 'click':
            return { action, ref };
        case 'fill':
        case 'select':
            if (typeof args.value !== 'string') {
                throw new Error(`${action} requires a string value.`);
            }
            return { action, ref, value: args.value };
        case 'set_checked':
            if (typeof args.checked !== 'boolean') {
                throw new Error('set_checked requires a boolean checked value.');
            }
            return { action, ref, checked: args.checked };
        case 'scroll':
            if (args.direction !== 'up' && args.direction !== 'down') {
                throw new Error('scroll direction must be up or down.');
            }
            return { action, ref, direction: args.direction };
    }
}

async function loadChatScroller(): Promise<ChatScroller> {
    // Import the running page instance: a static import would bundle a second SillyTavern runtime.
    const scriptUrl = '/script.js';
    const page = await import(scriptUrl /* webpackIgnore: true */) as {
        scrollChatSurfaceTo: ChatScroller;
    };
    return page.scrollChatSurfaceTo;
}

function requireInteractiveElement(element: Element): HTMLElement {
    if (!(element instanceof HTMLElement)) {
        throw new Error('app.interact does not support this element. Choose its enclosing control from app.snapshot, or use app.evaluate with a known API.');
    }
    if (isDisabled(element)) {
        throw new Error('The target is disabled. Check which condition enables it before trying again.');
    }
    if (element.closest('[inert]')) {
        throw new Error('This region is currently inactive and cannot receive input. Inspect the active panel or dialog instead.');
    }

    const hasModal = CSS.supports('selector(:modal)') && document.querySelector(':modal') !== null;
    if (hasModal && !element.closest(':modal')) {
        throw new Error('An open modal dialog blocks this target. Use app.snapshot to inspect the dialog, then operate or close it before returning to this control.');
    }
    return element;
}

/** Validate an action and capture its typed target. Only the returned function changes the UI. */
function prepareAction(
    request: Interaction,
    element: HTMLElement,
    semantics: Semantics,
    scrollChat: ChatScroller | undefined,
): () => boolean {
    switch (request.action) {
        case 'click': {
            const surface = requireClickSurface(element, semantics);
            return () => {
                surface.click();
                return true;
            };
        }
        case 'fill': {
            const supportedInput = element instanceof HTMLInputElement && FILLABLE_INPUT_TYPES.has(element.type);
            if (!(element instanceof HTMLTextAreaElement || supportedInput) || element.closest('.cm-editor')) {
                throw new Error('fill supports ordinary text/number inputs and textareas. For a complex editor, use app.evaluate with its editing API. Password and file inputs are not supported.');
            }
            if (element.readOnly || element.getAttribute('aria-readonly') === 'true') {
                throw new Error('The target is read-only. Find an editable control or inspect what makes this field read-only before trying again.');
            }
            requireInputSurface([element], semantics);

            return () => {
                element.focus({ preventScroll: true });
                setNativeInputValue(element, request.value);
                notifyValueChange(element);
                return true;
            };
        }
        case 'select': {
            if (!(element instanceof HTMLSelectElement) || element.multiple) {
                throw new Error('select supports standard single-select controls. For a custom dropdown, click to open it and use app.snapshot to inspect its options; for multi-select, use a known API.');
            }
            const option = Array.from(element.options).find(candidate => candidate.value === request.value);
            if (!option || option.disabled || option.parentElement?.matches('optgroup:disabled')) {
                throw new Error('This option is missing or disabled. Call app.snapshot with this control as root, then use an enabled option\'s exact value and the control\'s new ref.');
            }
            requireInputSurface([element], semantics);

            return () => {
                element.value = request.value;
                notifyValueChange(element);
                return true;
            };
        }
        case 'set_checked': {
            if (!isCheckableInput(element)) {
                throw new Error('set_checked supports standard checkbox and radio inputs. For a custom toggle, use click and verify the resulting state.');
            }
            if (element.type === 'radio' && !request.checked) {
                throw new Error('A radio cannot be unchecked directly. Select another radio in the group with checked=true.');
            }
            const surface = requireClickSurface(element, semantics);

            return () => {
                if (element.checked === request.checked) {
                    return false;
                }
                surface.click();
                return true;
            };
        }
        case 'scroll': {
            if (!semantics.isScrollable(element)) {
                throw new Error('This target cannot scroll vertically. Use app.snapshot to find a region marked scrollable=true, then scroll that region.');
            }
            requireInputSurface([element], semantics);

            return () => {
                const direction = request.direction === 'down' ? 1 : -1;
                const top = element.scrollTop + direction * element.clientHeight;
                if (scrollChat) {
                    scrollChat({ top, behavior: 'instant' });
                } else {
                    element.scrollTo({ top, behavior: 'instant' });
                }
                return true;
            };
        }
    }
}

function setNativeInputValue(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
    const prototype = element instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
    if (!descriptor?.set) {
        throw new Error('The native input value setter is unavailable.');
    }

    // Bypass React's instance setter so its input handler observes the change and updates state.
    descriptor.set.call(element, value);
}

function notifyValueChange(element: HTMLElement) {
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
}

function requireClickSurface(element: HTMLElement, semantics: Semantics): HTMLElement {
    const candidates = [element];
    if (isCheckableInput(element)) {
        candidates.push(...Array.from(element.labels ?? []));
    }
    return requireInputSurface(candidates, semantics);
}

function requireInputSurface(candidates: HTMLElement[], semantics: Semantics): HTMLElement {
    const surface = candidates.find(candidate => receivesInput(candidate, semantics));
    if (!surface) {
        throw new Error('No action was performed: the target is hidden, outside the visible area, or covered. Inspect the interface, then open its panel, scroll its region or close a covering panel as needed. Observe again before retrying.');
    }
    return surface;
}

function receivesInput(element: HTMLElement, semantics: Semantics): boolean {
    if (!semantics.isVisible(element)) {
        return false;
    }

    // ponytail: one point per client rect; add richer geometry only for observed partial-overlay failures.
    for (const rect of Array.from(element.getClientRects())) {
        const left = Math.max(0, rect.left);
        const right = Math.min(window.innerWidth, rect.right);
        const top = Math.max(0, rect.top);
        const bottom = Math.min(window.innerHeight, rect.bottom);
        if (right <= left || bottom <= top) {
            continue;
        }

        const hit = document.elementFromPoint((left + right) / 2, (top + bottom) / 2);
        if (hit && element.contains(hit)) {
            return true;
        }
    }
    return false;
}
