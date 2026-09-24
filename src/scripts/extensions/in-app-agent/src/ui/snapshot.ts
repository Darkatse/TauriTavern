import { createSemantics, describeLine, isSensitive, isTextRegion, preview, suppressesText } from './semantics';
import type { Semantics } from './semantics';

const MAX_NODES = 80;
const MAX_VISITS = 2_000;
const MAX_CONTENT_PREVIEW = 1_000;
const MAX_PAGE_CONTENT_PREVIEWS = 4_000;
export const MAX_DEPTH = 6;
const SNAPSHOT_SEQUENCE_KEY = 'tauritavern:in_app_agent_snapshot_sequence';

type ToolArguments = Record<string, TauriTavernJsonValue>;
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
    root: Element;
    maxDepth: number;
    next: TraversalPosition | null;
    dialogs: Element[];
    nextDialogIndex: number;
};
type Continuation = { token: string; traversal: Traversal };

/** One recent observation for the page. It survives drawer closure, not a Run change. */
export function createObservation() {
    let ownerRunId = '';
    let references = new Map<string, Element>();
    let continuation: Continuation | null = null;

    function enterRun(runId: string) {
        if (ownerRunId === runId) {
            return;
        }
        ownerRunId = runId;
        references.clear();
        clearCursor();
    }

    function clearCursor() {
        continuation = null;
    }

    function resolve(ref: string): Element {
        const element = references.get(ref);
        if (!element) {
            throw new Error('This ref is not in the current snapshot. Call app.snapshot and use a ref from its returned page.');
        }
        if (!element.isConnected || element.ownerDocument !== document) {
            throw new Error('The target is no longer in this page. Call app.snapshot to inspect the current interface before acting.');
        }
        return element;
    }

    function snapshot(args: ToolArguments) {
        // Resolve root/cursor against the previous page before replacing its references.
        const traversal = startOrResumeTraversal(args);
        const prefix = nextSnapshotPrefix();
        clearCursor();
        references = new Map();

        const page = readPage(traversal, prefix, references);
        if (page.hasMore) {
            const token = `${prefix}:next`;
            continuation = { token, traversal };
            return { tree: page.tree, truncated: true, nextCursor: token };
        }
        return { tree: page.tree, truncated: page.truncated };
    }

    function startOrResumeTraversal(args: ToolArguments): Traversal {
        const hasUnexpectedField = Object.keys(args).some(key => !['root', 'depth', 'cursor'].includes(key));
        if (hasUnexpectedField) {
            throw new Error('app.snapshot accepts root and depth, or cursor alone. Use {} for an overview.');
        }

        if (args.cursor !== undefined) {
            if (args.root !== undefined || args.depth !== undefined) {
                throw new Error('Pass cursor alone to continue reading. To inspect a region with root or depth, omit cursor.');
            }
            if (typeof args.cursor !== 'string' || args.cursor !== continuation?.token) {
                throw new Error('This cursor is invalid or expired. Call app.snapshot without cursor to observe the current interface.');
            }

            const traversal = continuation.traversal;
            const nextNode = traversal.next?.node;
            const nextNodeLeftRoot = nextNode && (!nextNode.isConnected || !traversal.root.contains(nextNode));
            if (!traversal.root.isConnected || nextNodeLeftRoot) {
                throw new Error('The observed region changed and cannot be continued. Call app.snapshot without cursor to observe it again.');
            }
            return traversal;
        }

        if (args.root !== undefined && typeof args.root !== 'string') {
            throw new Error('root must be a ref string from the latest app.snapshot page. Omit root for an overview.');
        }
        const depth = args.depth ?? 2;
        if (typeof depth !== 'number' || !Number.isInteger(depth) || depth < 1 || depth > MAX_DEPTH) {
            throw new Error(`depth must be an integer between 1 and ${MAX_DEPTH}.`);
        }

        const root = typeof args.root === 'string' ? resolve(args.root) : document.body;
        const dialogs = args.root === undefined
            ? Array.from(document.querySelectorAll('dialog[open]')).slice(0, MAX_NODES)
            : [];
        return {
            root,
            maxDepth: depth,
            next: { node: root, ancestors: [], depth: 0, suppressText: false },
            dialogs,
            nextDialogIndex: 0,
        };
    }

    return { enterRun, resolve, snapshot, clearCursor };
}

export type Observation = ReturnType<typeof createObservation>;

function nextSnapshotPrefix(): string {
    // Persist the sequence so reloading the page cannot reuse refs still present in conversation history.
    const next = Number(localStorage.getItem(SNAPSHOT_SEQUENCE_KEY) ?? 0) + 1;
    if (!Number.isSafeInteger(next) || next < 1) {
        throw new Error('Cannot create UI references: the saved snapshot counter is invalid. Use app.evaluate to inspect the app while this is resolved.');
    }
    localStorage.setItem(SNAPSHOT_SEQUENCE_KEY, String(next));
    return `s${next}`;
}

/** Render one bounded page and leave traversal.next at the first unvisited node. */
function readPage(traversal: Traversal, prefix: string, references: Map<string, Element>) {
    const semantics = createSemantics();
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
        const suffix = childrenOmitted ? ' [children omitted; use root to inspect]' : '';
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
        if (node instanceof Element) {
            const summarizedDialog = node !== traversal.root && traversal.dialogs.includes(node);
            if (semantics.excludesSubtree(node) || summarizedDialog) {
                childContext = null;
            } else {
                // Native options have no ordinary layout boxes, but are observable through their select.
                const selectOption = traversal.root instanceof HTMLSelectElement && node.matches('option,optgroup');
                const shown = selectOption || semantics.isVisible(node);
                const role = semantics.readRole(node, textPreview !== undefined);
                if (shown && (role || node === traversal.root)) {
                    const omitted = semantics.readOmissionReason(node);
                    const nestedSelect = node instanceof HTMLSelectElement && node !== traversal.root;
                    const depthLimitReached = depth >= traversal.maxDepth;
                    const hasChildren = node.firstChild !== null && !node.matches('input,textarea');
                    const childrenOmitted = hasChildren && (nestedSelect || depthLimitReached) && !omitted;
                    const ancestor: SemanticAncestor = { element: node, depth };
                    if (!textPreview && isTextRegion(node) && !omitted) {
                        ancestor.textPreview = { value: '', truncated: false };
                    }
                    appendElement(ancestor, childrenOmitted);

                    const stopHere = omitted || nestedSelect || depthLimitReached || node.matches('input,textarea,option');
                    if (stopHere) {
                        childContext = null;
                    } else {
                        childContext.depth = depth + 1;
                        childContext.ancestors = [...ancestors, ancestor];
                    }
                }
                if (childContext) {
                    childContext.suppressText ||= suppressesText(node, role) || isSensitive(node);
                }
            }
        } else if (node instanceof Text && !suppressText && !textPreview?.truncated && semantics.isTextVisible(node)) {
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
        if (node instanceof HTMLDetailsElement && !node.open) {
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
        const parentIsClosedDetails = parent instanceof HTMLDetailsElement && !parent.open;
        if (node.nextSibling && !parentIsClosedDetails) {
            // Climbing out of a semantic region removes it from the next sibling's context.
            const ancestors = position.ancestors.filter(ancestor => ancestor.element.contains(parent));
            const lastAncestor = ancestors.at(-1);
            const suppressText = ancestors.some(ancestor =>
                suppressesText(ancestor.element, semantics.readRole(ancestor.element)) || isSensitive(ancestor.element),
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
