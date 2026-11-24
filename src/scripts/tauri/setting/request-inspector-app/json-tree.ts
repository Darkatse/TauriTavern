import { countMatches } from './search';

export type TreeKind = 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null';

export type TreeNode = {
    /** RFC 6901 JSON Pointer from the root. */
    pointer: string;
    parent: string | null;
    depth: number;
    /** Key or index; `null` for a root that is not a container. */
    label: string | null;
    value: unknown;
    kind: TreeKind;
    /** Child count of a container, length of a string. */
    size: number;
    expandable: boolean;
    /** The array this node is an item of. */
    siblings: unknown[] | null;
};

export type TreeLine =
    | { type: 'node'; node: TreeNode; expanded: boolean }
    | { type: 'more'; pointer: string; depth: number; rest: number };

/** Children shown per step of a container. */
export const PAGE = 200;
const INLINE_STRING = 80;
const SUMMARY_ENTRIES = 3;
const SUMMARY_STRING = 24;
/** Scalars read from one array item when it is compared with its siblings. */
const SUMMARY_LEAVES = 64;

function pointerOf(parent: string, key: string | number): string {
    return `${parent}/${String(key).replaceAll('~', '~0').replaceAll('/', '~1')}`;
}

/** Every container above `pointer`, root first. */
export function ancestorsOf(pointer: string): string[] {
    const ancestors: string[] = [];
    for (let end = pointer.lastIndexOf('/'); end >= 0; end = pointer.lastIndexOf('/', end - 1)) {
        ancestors.unshift(pointer.slice(0, end));
        if (end === 0) {
            break;
        }
    }
    return ancestors;
}

function kindOf(value: unknown): TreeKind {
    if (value === null || value === undefined) {
        return 'null';
    }
    if (Array.isArray(value)) {
        return 'array';
    }
    const type = typeof value;
    return type === 'object' || type === 'string' || type === 'number' || type === 'boolean' ? type : 'null';
}

function childrenOf(value: unknown): [key: string, value: unknown][] {
    if (Array.isArray(value)) {
        return value.map((child, index) => [String(index), child as unknown]);
    }
    return value !== null && typeof value === 'object' ? Object.entries(value) : [];
}

function makeNode(
    value: unknown,
    pointer: string,
    parent: string | null,
    depth: number,
    label: string | null,
    siblings: unknown[] | null,
): TreeNode {
    const kind = kindOf(value);
    const container = kind === 'object' || kind === 'array';
    const size = container ? childrenOf(value).length : typeof value === 'string' ? value.length : 0;
    return {
        pointer, parent, depth, label, value, kind, size, siblings,
        expandable: container ? size > 0 : typeof value === 'string' && isLongString(value),
    };
}

/** A string too long for one line of the tree; it opens into a block. */
function isLongString(value: string): boolean {
    return value.length > INLINE_STRING || value.includes('\n');
}

/**
 * The lines to draw. A container root is not drawn itself. With `keep`, only
 * those nodes are drawn and each of them is open.
 */
export function visibleLines(
    root: unknown,
    expanded: ReadonlySet<string>,
    limits: ReadonlyMap<string, number>,
    keep: ReadonlySet<string> | null,
): TreeLine[] {
    const lines: TreeLine[] = [];
    const walkChildren = (value: unknown, pointer: string, depth: number) => {
        const children = childrenOf(value)
            .map(([key, child]) => ({ key, child, pointer: pointerOf(pointer, key) }))
            .filter(child => !keep || keep.has(child.pointer));
        const limit = limits.get(pointer) ?? PAGE;
        const siblings = Array.isArray(value) ? value : null;
        for (const child of children.slice(0, limit)) {
            walk(child.child, child.pointer, pointer, depth, child.key, siblings);
        }
        if (children.length > limit) {
            lines.push({ type: 'more', pointer, depth, rest: children.length - limit });
        }
    };
    const walk = (
        value: unknown,
        pointer: string,
        parent: string | null,
        depth: number,
        label: string | null,
        siblings: unknown[] | null,
    ) => {
        const node = makeNode(value, pointer, parent, depth, label, siblings);
        const open = node.expandable && (keep ? keep.has(pointer) : expanded.has(pointer));
        lines.push({ type: 'node', node, expanded: open });
        if (open && node.kind !== 'string') {
            walkChildren(value, pointer, depth + 1);
        }
    };

    const kind = kindOf(root);
    if (kind === 'object' || kind === 'array') {
        walkChildren(root, '', 0);
    } else {
        walk(root, '', null, 0, null, null);
    }
    return lines;
}

/** Nodes whose key or string value holds the query, with the containers leading to them. */
export function searchTree(root: unknown, needle: string): { keep: Set<string>; count: number } {
    const keep = new Set<string>();
    let count = 0;
    const walk = (value: unknown, pointer: string, label: string | null): boolean => {
        const found = countMatches(label ?? '', needle) + (typeof value === 'string' ? countMatches(value, needle) : 0);
        count += found;
        let inside = false;
        for (const [key, child] of childrenOf(value)) {
            inside = walk(child, pointerOf(pointer, key), key) || inside;
        }
        if (found > 0 || inside) {
            keep.add(pointer);
            return true;
        }
        return false;
    };
    walk(root, '', null);
    return { keep, count };
}

/** The scalars inside a value by dotted path, in document order. */
function scalarLeaves(value: unknown, path = '', leaves: [path: string, value: unknown][] = []): [string, unknown][] {
    for (const [key, child] of childrenOf(value)) {
        if (leaves.length >= SUMMARY_LEAVES) {
            break;
        }
        const childPath = path ? `${path}.${key}` : key;
        if (child !== null && typeof child === 'object') {
            scalarLeaves(child, childPath, leaves);
        } else {
            leaves.push([childPath, child]);
        }
    }
    return leaves;
}

const sharedLeaves = new WeakMap<unknown[], Map<string, unknown>>();

/** The scalars that every container among an array's items holds with the same value. */
function sharedBy(siblings: unknown[]): Map<string, unknown> {
    let shared = sharedLeaves.get(siblings);
    if (!shared) {
        const [first, ...rest] = siblings.filter(item => item !== null && typeof item === 'object');
        shared = new Map(rest.length > 0 ? scalarLeaves(first) : []);
        for (const sibling of rest) {
            const own = new Map(scalarLeaves(sibling));
            for (const [path, value] of shared) {
                if (!own.has(path) || own.get(path) !== value) {
                    shared.delete(path);
                }
            }
        }
        sharedLeaves.set(siblings, shared);
    }
    return shared;
}

/**
 * What a closed container holds, read without knowing the wire format. An item
 * of an array is told apart by what its siblings do not share, such as the
 * text one stream event adds.
 */
export function summarize(value: unknown, items: (count: number) => string, siblings: unknown[] | null): string {
    if (Array.isArray(value)) {
        return items(value.length);
    }
    const shared = siblings && sharedBy(siblings);
    const own = shared ? scalarLeaves(value).filter(([path, leaf]) => !shared.has(path) || shared.get(path) !== leaf) : [];
    const entries = own.length > 0
        ? own
        : childrenOf(value).filter(([, child]) => child === null || typeof child !== 'object');
    const shown = entries
        .slice(0, SUMMARY_ENTRIES)
        .map(([key, leaf]) => `${key}: ${typeof leaf === 'string' ? JSON.stringify(clip(leaf, SUMMARY_STRING)) : String(leaf)}`);
    return `{ ${[...shown, ...(childrenOf(value).length > shown.length ? ['…'] : [])].join(', ')} }`;
}

export function clip(text: string, length: number): string {
    return text.length > length ? `${text.slice(0, length)}…` : text;
}

/** The start of a long string, as its closed line shows it. */
export function inlineString(value: string): string {
    const end = value.indexOf('\n');
    return clip(end < 0 ? value : value.slice(0, end), INLINE_STRING);
}

/** How many children a container must show for the child at `pointer` to be drawn. */
export function limitFor(pointer: string): [parent: string, limit: number] | null {
    const cut = pointer.lastIndexOf('/');
    const index = Number(pointer.slice(cut + 1));
    return cut >= 0 && Number.isSafeInteger(index) ? [pointer.slice(0, cut), (Math.floor(index / PAGE) + 1) * PAGE] : null;
}
