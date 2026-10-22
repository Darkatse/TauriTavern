import type { ToolArguments } from './types.ts';
import { appToolAliases } from './names.ts';

const { snapshot: SNAPSHOT } = appToolAliases;

export type ElementTarget = { ref: string } | { selector: string; root?: string | undefined };

export function parseTarget(args: ToolArguments): ElementTarget {
    const hasRef = args.ref !== undefined;
    const hasSelector = args.selector !== undefined;
    if (hasRef === hasSelector) {
        throw new Error('Provide exactly one of ref or selector. Use a ref from the latest snapshot, or a known CSS selector.');
    }
    if (hasRef) {
        if (typeof args.ref !== 'string' || !args.ref.trim()) {
            throw new Error(`ref must be a non-empty string copied from the latest ${SNAPSHOT} page.`);
        }
        if (args.root !== undefined) {
            throw new Error('root is only used with selector. A ref already identifies its element and page; omit root.');
        }
        return { ref: args.ref };
    }
    if (typeof args.selector !== 'string' || !args.selector.trim()) {
        throw new Error(`selector must be a non-empty CSS selector for a known target. Use ${SNAPSHOT} if the target is unclear.`);
    }
    if (args.root !== undefined && typeof args.root !== 'string') {
        throw new Error('root must be a ref from the latest snapshot. Omit root to search the main document.');
    }
    return { selector: args.selector, root: args.root };
}
