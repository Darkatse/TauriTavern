import { contains, intersects, toTopViewport } from './geometry.ts';
import type { Rect } from './geometry.ts';
import { parseTarget } from './target.ts';
import type { ElementTarget } from './target.ts';
import type { CaptureLayout } from './capture.ts';
import { createSemantics, describeLine, isTextRegion, preview, suppressesText } from './semantics.ts';
import type { ObservationOptions, Semantics } from './semantics.ts';
import type { ToolArguments } from './types.ts';
import { isElement, isHtmlTag, isText, documentWindow, mainDocumentScope, requireCurrentScope } from './document.ts';
import type { DocumentScope } from './document.ts';
import { appToolAliases } from './names.ts';

const { snapshot: SNAPSHOT, interact: INTERACT, capture: CAPTURE, evaluate: EVALUATE } = appToolAliases;

const MAX_NODES = 80;
const MAX_VISITS = 2_000;
const MAX_CONTENT_PREVIEW = 1_000;
const MAX_PAGE_CONTENT_PREVIEWS = 4_000;
export const MAX_DEPTH = 6;
const SNAPSHOT_SEQUENCE_KEY = 'tauritavern:in_app_agent_snapshot_sequence';

type ResolvedTarget = { element: Element; scope: DocumentScope };
type TextPreview = { value: string; truncated: boolean };
type SemanticAncestor = { element: Element; depth: number; textPreview?: TextPreview };
type SnapshotLine = { text: string; preview?: TextPreview | undefined };
type TraversalContext = {
    ancestors: SemanticAncestor[];
    depth: number;
    suppressText: boolean;
};
type TraversalPosition = TraversalContext & { node: Node };
type Traversal = {
    scope: DocumentScope;
    root: Element;
    maxDepth: number;
    next: TraversalPosition | null;
    dialogs: Element[];
    nextDialogIndex: number;
};
type Continuation = { token: string; traversal: Traversal };

/** One recent observation for the page. It survives drawer closure, not a Run change. */
export function createObservation(options: ObservationOptions = {}) {
    let ownerRunId = '';
    let references = new Map<string, Element>();
    let continuation: Continuation | null = null;
    let scope = mainDocumentScope();
    const semantics = () => createSemantics(options);

    function requireScope(scope: DocumentScope) {
        requireCurrentScope(scope);
        const current = semantics();
        for (const frame of scope.frames) current.requireFrameVisible(frame);
    }

    function enterRun(runId: string) {
        if (ownerRunId === runId) {
            return;
        }
        ownerRunId = runId;
        references.clear();
        scope = mainDocumentScope();
        clearCursor();
    }

    function clearCursor() {
        continuation = null;
    }

    function resolveRef(ref: string): ResolvedTarget {
        requireScope(scope);
        return { element: resolveRefElement(ref), scope };
    }

    function lookupRef(ref: string): { element: Element } | { problem: string } {
        const element = references.get(ref);
        if (!element) return { problem: `This ref is not in the current snapshot. Call ${SNAPSHOT} and use a ref from its returned page.` };
        if (!element.isConnected || element.ownerDocument !== scope.document) {
            return { problem: `The target is no longer in this page. Call ${SNAPSHOT} to inspect the current interface before acting.` };
        }
        if (!semantics().isWithinObservedContent(element)) {
            return { problem: `The target is inside a hidden or omitted region. Call ${SNAPSHOT} to find its visible panel or restore control before acting.` };
        }
        return { element };
    }

    function resolveRefElement(ref: string): Element {
        const found = lookupRef(ref);
        if ('problem' in found) throw new Error(found.problem);
        return found.element;
    }

    /** The refs of the latest snapshot page, in the order the page lists them. */
    function refs(): string[] {
        return [...references.keys()];
    }

    /** Locating an element does not replace snapshot refs or change the observed document. */
    function resolve(target: ElementTarget): ResolvedTarget {
        if ('ref' in target) return resolveRef(target.ref);
        const current = semantics();
        const base = target.root === undefined
            ? { element: document.documentElement, scope: mainDocumentScope() }
            : resolveRef(target.root);
        const region = enterFrame(base, current);
        return { element: selectElement(region.element, target.selector, current), scope: region.scope };
    }

    function captureLayout(args: ToolArguments): CaptureLayout {
        if (Object.keys(args).some(key => !['ref', 'selector', 'root', 'marks'].includes(key))) {
            throw new Error(`${CAPTURE} accepts ref, selector, root and marks.`);
        }
        if (args.marks !== undefined && typeof args.marks !== 'boolean') {
            throw new Error('marks must be a boolean.');
        }
        const target = args.ref === undefined && args.selector === undefined && args.root === undefined
            ? null : resolve(parseTarget(args));
        const view = documentWindow(document);
        const viewport = { width: view.innerWidth, height: view.innerHeight };
        const targetRect = target ? captureRect(target) : null;
        if (target && !targetRect) {
            throw new Error(`The element is outside the visible part of the page. Scroll the region that contains it with ${INTERACT}, then capture again.`);
        }
        const crop = targetRect?.rect ?? null;
        const area = crop ?? { x: 0, y: 0, ...viewport };
        const current = semantics();
        const masks: Rect[] = [];
        function walk(element: Element, region: DocumentScope) {
            if (current.isMasked(element)) {
                const visible = captureRect({ element, scope: region });
                if (visible && intersects(visible.rect, area)) masks.push(visible.rect);
                return;
            }
            for (const child of element.children) walk(child, region);
            if (isHtmlTag(element, 'iframe')) {
                const child = enterableFrame(element, region, current);
                if (child) walk(child.document.documentElement, child);
            }
        }
        walk(document.documentElement, mainDocumentScope());
        const marks: CaptureLayout['marks'] = [];
        let markPage: string | null = null;
        let markTotal = 0;
        if (args.marks === true) {
            const pageRefs = refs();
            if (pageRefs.length === 0) throw new Error(`Call ${SNAPSHOT} first: marks number the refs of your latest snapshot page.`);
            requireScope(scope);
            markTotal = pageRefs.length;
            markPage = refParts(pageRefs[0]!).page;
            for (const ref of pageRefs) {
                const found = lookupRef(ref);
                if ('problem' in found) continue;
                const visible = captureRect({ element: found.element, scope });
                if (!visible || !intersects(visible.rect, area) || contains(visible.rect, area)) continue;
                marks.push({ label: refParts(ref).label, rect: visible.rect });
            }
        }
        return { viewport, crop, cropClipped: targetRect?.clipped ?? false, masks, marks, markPage, markTotal };
    }

    function snapshot(args: ToolArguments) {
        // Resolve root/cursor against the previous page before replacing its references.
        const traversal = startOrResumeTraversal(args);
        scope = traversal.scope;
        const prefix = nextSnapshotPrefix();
        clearCursor();
        references = new Map();

        const page = readPage(traversal, prefix, references, semantics());
        if (page.hasMore) {
            const token = `${prefix}:next`;
            continuation = { token, traversal };
            return { tree: page.tree, truncated: true, nextCursor: token };
        }
        return { tree: page.tree, truncated: page.truncated };
    }

    function startOrResumeTraversal(args: ToolArguments): Traversal {
        const hasUnexpectedField = Object.keys(args).some(key => !['root', 'selector', 'depth', 'cursor'].includes(key));
        if (hasUnexpectedField) {
            throw new Error(`${SNAPSHOT} accepts root, selector and depth, or cursor alone. Use {} for an overview.`);
        }

        if (args.cursor !== undefined) {
            if (args.root !== undefined || args.selector !== undefined || args.depth !== undefined) {
                throw new Error('Pass cursor alone to continue reading. To inspect a region with root, selector or depth, omit cursor.');
            }
            if (typeof args.cursor !== 'string' || args.cursor !== continuation?.token) {
                throw new Error(`This cursor is invalid or expired. Call ${SNAPSHOT} without cursor to observe the current interface.`);
            }

            const traversal = continuation.traversal;
            requireScope(traversal.scope);
            const nextNode = traversal.next?.node;
            const nextNodeLeftRoot = nextNode && (!nextNode.isConnected || !traversal.root.contains(nextNode));
            if (!traversal.root.isConnected || nextNodeLeftRoot) {
                throw new Error(`The observed region changed and cannot be continued. Call ${SNAPSHOT} without cursor to observe it again.`);
            }
            return traversal;
        }

        if (args.root !== undefined && typeof args.root !== 'string') {
            throw new Error(`root must be a ref string from the latest ${SNAPSHOT} page. Omit root for an overview.`);
        }
        if (args.selector !== undefined && (typeof args.selector !== 'string' || !args.selector.trim())) {
            throw new Error('selector must be a non-empty CSS selector for a known target. Omit it to explore with an overview.');
        }
        const depth = args.depth ?? 2;
        if (typeof depth !== 'number' || !Number.isInteger(depth) || depth < 1 || depth > MAX_DEPTH) {
            throw new Error(`depth must be an integer between 1 and ${MAX_DEPTH}.`);
        }

        let target: ResolvedTarget = { element: document.documentElement, scope: mainDocumentScope() };
        if (typeof args.selector === 'string') {
            target = resolve({ selector: args.selector, root: args.root });
        } else if (args.root !== undefined) {
            target = resolveRef(args.root);
        }
        const { element: root, scope: nextScope } = enterFrame(target, semantics());
        const isDocumentRoot = root === nextScope.document.documentElement || root === nextScope.document.body;
        const dialogs = isDocumentRoot
            ? Array.from(nextScope.document.querySelectorAll('dialog[open]')).slice(0, MAX_NODES)
            : [];
        return {
            scope: nextScope,
            root,
            maxDepth: depth,
            next: { node: root, ancestors: [], depth: 0, suppressText: false },
            dialogs,
            nextDialogIndex: 0,
        };
    }

    return { enterRun, refs, resolve, snapshot, captureLayout, clearCursor, createSemantics: semantics };
}

export type Observation = ReturnType<typeof createObservation>;

function refParts(ref: string): { page: string; label: string } {
    const colon = ref.indexOf(':');
    return { page: ref.slice(0, colon), label: ref.slice(colon + 1) };
}

function captureRect({ element, scope }: ResolvedTarget): { rect: Rect; clipped: boolean } | null {
    const mapped = toTopViewport(element.getBoundingClientRect(), scope);
    if ('outside' in mapped) return null;
    const { bounds, clipped } = mapped;
    return { rect: { x: bounds.left, y: bounds.top, width: bounds.right - bounds.left, height: bounds.bottom - bounds.top }, clipped };
}

function enterableFrame(frame: HTMLIFrameElement, scope: DocumentScope, semantics: Semantics): DocumentScope | null {
    if (semantics.readOmissionReason(frame)) return null;
    return { document: frame.contentDocument!, frames: [...scope.frames, frame] };
}

function enterFrame(target: ResolvedTarget, semantics: Semantics): ResolvedTarget {
    const { element, scope } = target;
    if (isHtmlTag(element, 'iframe')) {
        semantics.requireFrameVisible(element);
        const child = enterableFrame(element, scope, semantics);
        if (child) return { element: child.document.documentElement, scope: child };
    }
    return target;
}

/** Snapshot and interaction share one rule for selecting an observable element. */
function selectElement(root: Element, selector: string, semantics: Semantics): Element {
    if (semantics.readOmissionReason(root)) {
        throw new Error('This region\'s contents are omitted and cannot be searched. Inspect its snapshot for the reason.');
    }
    const container = root === root.ownerDocument.documentElement ? root.ownerDocument : root;
    let matches: NodeListOf<Element>;
    try {
        matches = container.querySelectorAll(selector);
    } catch {
        throw new Error('Invalid CSS selector. Use a valid selector for a known target, or omit selector to explore the interface.');
    }
    let selected: Element | undefined;
    for (const element of matches) {
        if (!semantics.isVisible(element) || !semantics.isWithinObservedContent(element)) continue;
        if (selected) {
            throw new Error('More than one observable element matches selector. Use a more specific selector or a narrower root; no target was selected.');
        }
        selected = element;
    }
    if (!selected) {
        throw new Error(matches.length
            ? 'Matching elements are hidden, have no visible size, or belong to omitted content. Observe the main page to find their visible panel or restore control.'
            : 'No element matches selector in this region. Check the selector and its containing page, or omit selector to explore. Selectors do not search inside other iframes.');
    }
    return selected;
}

function nextSnapshotPrefix(): string {
    // Persist the sequence so reloading the page cannot reuse refs still present in conversation history.
    const next = Number(localStorage.getItem(SNAPSHOT_SEQUENCE_KEY) ?? 0) + 1;
    if (!Number.isSafeInteger(next) || next < 1) {
        throw new Error(`Cannot create UI references: the saved snapshot counter is invalid. Use ${EVALUATE} to inspect the app while this is resolved.`);
    }
    localStorage.setItem(SNAPSHOT_SEQUENCE_KEY, String(next));
    return `s${next}`;
}

/** Render one bounded page and leave traversal.next at the first unvisited node. */
function readPage(traversal: Traversal, prefix: string, references: Map<string, Element>, semantics: Semantics) {
    // Descendants fill their region's preview during this same walk; format lines after the page is read.
    const lines: SnapshotLine[] = [];
    const textBudget = { remaining: MAX_PAGE_CONTENT_PREVIEWS };
    const emittedElements = new Set<Element>();
    let truncated = false;
    let visits = 0;

    function appendElement({ element, depth, textPreview }: SemanticAncestor, childrenOmitted = false) {
        truncated ||= childrenOmitted;
        if (emittedElements.has(element)) {
            return;
        }

        const ref = `${prefix}:e${references.size + 1}`;
        references.set(ref, element);
        emittedElements.add(element);
        // A continuation repeats its ancestor's bounded preview, which also consumes this page's budget.
        if (textPreview) textBudget.remaining -= JSON.stringify(textPreview.value).length - 2;

        const description = semantics.describeElement(element);
        const hasTextPreview = Object.keys(description).some(key => key.endsWith('Truncated'));
        truncated ||= hasTextPreview || Boolean(description.omitted);

        const indent = '  '.repeat(depth);
        const suffix = childrenOmitted
            ? (isHtmlTag(element, 'iframe') ? ' [contents omitted; use root to enter]' : ' [children omitted; use root to inspect]') : '';
        lines.push({ text: `${indent}${describeLine(description)} [ref=${ref}]${suffix}`, preview: textPreview });
    }

    // Dialogs are appended at the end of the DOM. Expose their roots before a large background.
    while (traversal.nextDialogIndex < traversal.dialogs.length && lines.length < MAX_NODES) {
        const dialog = traversal.dialogs[traversal.nextDialogIndex++];
        if (dialog && dialog.isConnected && semantics.isVisible(dialog)) {
            appendElement({ element: dialog, depth: 0 }, true);
        }
    }

    // A continuation has fresh refs. Repeat its root/path so descendants remain actionable.
    if (traversal.next && traversal.next.node !== traversal.root) {
        for (const ancestor of traversal.next.ancestors) {
            appendElement(ancestor);
        }
    }

    while (traversal.next && lines.length < MAX_NODES && visits < MAX_VISITS) {
        const position = traversal.next;
        const { node, ancestors, depth, suppressText } = position;
        const textPreview = ancestors.find(ancestor => ancestor.textPreview)?.textPreview;
        visits++;

        // Layout wrappers pass this context through; only emitted semantic elements add a level.
        let childContext: TraversalContext | null = { ancestors, depth, suppressText };
        if (isElement(node)) {
            const summarizedDialog = node !== traversal.root && traversal.dialogs.includes(node);
            if (semantics.excludesSubtree(node) || summarizedDialog) {
                childContext = null;
            } else {
                // Native options have no ordinary layout boxes, but are observable through their select.
                const selectOption = isHtmlTag(traversal.root, 'select') && node.matches('option,optgroup');
                const shown = selectOption || semantics.isVisible(node);
                const role = semantics.readRole(node, textPreview !== undefined);
                if (shown && (role || node === traversal.root)) {
                    const omitted = semantics.readOmissionReason(node);
                    const nestedSelect = isHtmlTag(node, 'select') && node !== traversal.root;
                    const frameBoundary = isHtmlTag(node, 'iframe');
                    const depthLimitReached = depth >= traversal.maxDepth;
                    const hasChildren = node.firstChild !== null && !node.matches('input,textarea');
                    const childrenOmitted = !omitted && (frameBoundary || (hasChildren && (nestedSelect || depthLimitReached)));
                    const ancestor: SemanticAncestor = { element: node, depth };
                    if (!textPreview && isTextRegion(node) && !omitted) {
                        ancestor.textPreview = { value: '', truncated: false };
                    }
                    appendElement(ancestor, childrenOmitted);

                    const stopHere = omitted || frameBoundary || nestedSelect || depthLimitReached || node.matches('input,textarea,option');
                    if (stopHere) {
                        childContext = null;
                    } else {
                        childContext.depth = depth + 1;
                        childContext.ancestors = [...ancestors, ancestor];
                    }
                }
                if (childContext) {
                    childContext.suppressText ||= suppressesText(node, role) || semantics.isSensitive(node);
                }
            }
        } else if (isText(node) && !suppressText && !textPreview?.truncated && semantics.isTextVisible(node)) {
            // Read this text node, never a container's potentially huge textContent.
            if (textPreview) {
                appendContentText(textPreview, node.data, textBudget);
            } else {
                const bounded = preview(node.data);
                if (bounded.value.trim()) {
                    const suffix = bounded.truncated ? ' [preview]' : '';
                    lines.push({ text: `${'  '.repeat(depth)}text ${JSON.stringify(bounded.value)}${suffix}` });
                }
                truncated ||= bounded.truncated;
            }
        }

        traversal.next = advance(position, traversal.root, childContext, semantics);
    }

    return {
        tree: lines.map(formatLine).join('\n'),
        truncated: truncated || lines.some(line => line.preview?.truncated),
        hasMore: traversal.next !== null || traversal.nextDialogIndex < traversal.dialogs.length,
    };
}

/** One budget per message body, retained across pagination regardless of its DOM formatting. */
function appendContentText(target: TextPreview, text: string, budget: { remaining: number }) {
    const separator = target.value ? ' ' : '';
    const size = JSON.stringify(target.value).length - 2;
    const remaining = Math.min(MAX_CONTENT_PREVIEW - size, budget.remaining) - separator.length;
    const bounded = preview(text, remaining);
    if (bounded.value.trim()) target.value += separator + bounded.value.trim();
    budget.remaining -= JSON.stringify(target.value).length - 2 - size;
    target.truncated = bounded.truncated;
}

function formatLine(line: SnapshotLine): string {
    if (!line.preview || (!line.preview.value && !line.preview.truncated)) return line.text;
    const shortened = line.preview.truncated ? ' textTruncated=true' : '';
    return `${line.text} text=${JSON.stringify(line.preview.value)}${shortened}`;
}

/** Walk the live DOM; null childContext skips descendants without losing the next sibling. */
function advance(
    position: TraversalPosition,
    root: Element,
    childContext: TraversalContext | null,
    semantics: Semantics,
): TraversalPosition | null {
    let node = position.node;
    if (childContext) {
        let child = node.firstChild;
        if (isElement(node) && isHtmlTag(node, 'details') && !node.open) {
            child = node.querySelector(':scope > summary');
        }
        if (child) {
            return { node: child, ...childContext };
        }
    }

    while (node !== root) {
        const parent = node.parentElement;
        if (!parent) {
            return null;
        }
        const parentIsClosedDetails = isHtmlTag(parent, 'details') && !parent.open;
        if (node.nextSibling && !parentIsClosedDetails) {
            // Climbing out of a semantic region removes it from the next sibling's context.
            const ancestors = position.ancestors.filter(ancestor => ancestor.element.contains(parent));
            const lastAncestor = ancestors.at(-1);
            const suppressText = ancestors.some(ancestor =>
                suppressesText(ancestor.element, semantics.readRole(ancestor.element)) || semantics.isSensitive(ancestor.element),
            );
            return {
                node: node.nextSibling,
                ancestors,
                depth: lastAncestor ? lastAncestor.depth + 1 : 0,
                suppressText,
            };
        }
        node = parent;
    }
    return null;
}
