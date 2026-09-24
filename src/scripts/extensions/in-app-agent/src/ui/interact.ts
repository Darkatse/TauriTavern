import { createSemantics, isCheckableInput, isDisabled } from './semantics';
import type { Semantics } from './semantics';
import type { Observation } from './snapshot';
import { documentWindow, isHTMLElement, isHtmlTag, isInScope } from './document';
import type { DocumentScope } from './document';
import { inputPoint } from './geometry';
import type { InteractionPoint } from './geometry';
export type { InteractionPoint } from './geometry';

type ToolArguments = Record<string, TauriTavernJsonValue>;
type Interaction =
    | { action: 'click'; ref: string }
    | { action: 'fill'; ref: string; value: string }
    | { action: 'select'; ref: string; value: string }
    | { action: 'set_checked'; ref: string; checked: boolean }
    | { action: 'scroll'; ref: string; direction: 'up' | 'down' };
type ChatScroller = (options: ScrollToOptions) => void;
type InteractionFeedback = (point: InteractionPoint) => void;

const ACTION_FIELDS: Record<Interaction['action'], string[]> = {
    click: [],
    fill: ['value'],
    select: ['value'],
    set_checked: ['checked'],
    scroll: ['direction'],
};
const FILLABLE_INPUT_TYPES = new Set(['text', 'search', 'email', 'url', 'tel', 'number']);

export async function interact(args: ToolArguments, observation: Observation, signal: AbortSignal, feedback?: InteractionFeedback) {
    const request = parseInteraction(args);
    let scrollChat: ChatScroller | undefined;

    if (request.action === 'scroll' && observation.resolve(request.ref) === document.getElementById('chat')) {
        scrollChat = await loadChatScroller();
    }

    // Loading the page module can yield. Resolve again before checking or changing the target.
    if (signal.aborted) {
        throw new Error('The operation was cancelled before acting.');
    }
    const element = requireInteractiveElement(observation.resolve(request.ref), observation.scope);
    const scope = observation.scope;
    const apply = prepareAction(request, element, scope, scrollChat, feedback);
    observation.clearCursor();

    // Preparation only checks the target and signals visual feedback. apply changes application state.
    try {
        const dispatched = apply();
        const observed = isInScope(element, scope)
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

function requireInteractiveElement(element: Element, scope: DocumentScope): HTMLElement {
    if (!isHTMLElement(element)) {
        throw new Error('app.interact does not support this element. Choose its enclosing control from app.snapshot, or use app.evaluate with a known API.');
    }
    for (const target of [...scope.frames, element]) {
        if (isDisabled(target)) {
            throw new Error('The target or its containing page is disabled. Check which condition enables it before trying again.');
        }
        if (target.closest('[inert]')) {
            throw new Error('This region is currently inactive and cannot receive input. Call app.snapshot with {} to inspect the active panel or dialog.');
        }
        const doc = target.ownerDocument;
        const hasModal = documentWindow(doc).CSS.supports('selector(:modal)') && doc.querySelector(':modal') !== null;
        if (hasModal && !target.closest(':modal')) {
            throw new Error('An open modal dialog blocks this target. Observe the dialog and operate or close it first. For a dialog outside this embedded page, call app.snapshot with {} to inspect the main page.');
        }
    }
    return element;
}

/** Validate an action and capture its typed target. Only the returned function changes the control. */
function prepareAction(
    request: Interaction,
    element: HTMLElement,
    scope: DocumentScope,
    scrollChat: ChatScroller | undefined,
    feedback?: InteractionFeedback,
): () => boolean {
    const semantics = createSemantics();
    const requireSurface = (candidates: HTMLElement[]) => requireInputSurface(candidates, scope, semantics, feedback);
    switch (request.action) {
        case 'click': {
            const surface = requireSurface(clickCandidates(element));
            return () => {
                surface.click();
                return true;
            };
        }
        case 'fill': {
            const supportedInput = isHtmlTag(element, 'input') && FILLABLE_INPUT_TYPES.has(element.type);
            if (!(isHtmlTag(element, 'textarea') || supportedInput) || element.closest('.cm-editor')) {
                throw new Error('fill supports ordinary text/number inputs and textareas. For a complex editor, use app.evaluate with its editing API. Password and file inputs are not supported.');
            }
            if (element.readOnly || element.getAttribute('aria-readonly') === 'true') {
                throw new Error('The target is read-only. Find an editable control or inspect what makes this field read-only before trying again.');
            }
            requireSurface([element]);

            return () => {
                element.focus({ preventScroll: true });
                setNativeInputValue(element, request.value);
                notifyValueChange(element);
                return true;
            };
        }
        case 'select': {
            if (!isHtmlTag(element, 'select') || element.multiple) {
                throw new Error('select supports standard single-select controls. For a custom dropdown, click to open it and use app.snapshot to inspect its options; for multi-select, use a known API.');
            }
            const option = Array.from(element.options).find(candidate => candidate.value === request.value);
            if (!option || option.disabled || option.parentElement?.matches('optgroup:disabled')) {
                throw new Error('This option is missing or disabled. Call app.snapshot with this control as root, then use an enabled option\'s exact value and the control\'s new ref.');
            }
            requireSurface([element]);

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
            const surface = requireSurface(clickCandidates(element));

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
            requireSurface([element]);

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
    const view = documentWindow(element.ownerDocument);
    const prototype = isHtmlTag(element, 'textarea') ? view.HTMLTextAreaElement.prototype : view.HTMLInputElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
    if (!descriptor?.set) {
        throw new Error('The native input value setter is unavailable.');
    }

    // Bypass React's instance setter so its input handler observes the change and updates state.
    descriptor.set.call(element, value);
}

function notifyValueChange(element: HTMLElement) {
    const { Event } = documentWindow(element.ownerDocument);
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
}

function clickCandidates(element: HTMLElement): HTMLElement[] {
    const candidates = [element];
    if (isCheckableInput(element)) {
        candidates.push(...Array.from(element.labels ?? []));
    }
    return candidates;
}

function requireInputSurface(candidates: HTMLElement[], scope: DocumentScope, semantics: Semantics, feedback?: InteractionFeedback): HTMLElement {
    for (const candidate of candidates) {
        const point = inputPoint(candidate, scope, semantics);
        if (point) {
            feedback?.(point);
            return candidate;
        }
    }
    throw new Error('No action was performed: the target is hidden, outside the visible area, or covered. Scroll its region or close a covering panel, then observe again. For an obstruction outside this embedded page, call app.snapshot with {} to inspect the main page.');
}
