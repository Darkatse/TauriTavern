import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';

import type { Translate } from './RequestInspectorContract';
import { Highlight, TextBlock } from './TextBlock';
import { fill, formatCount } from './format';
import {
    PAGE, ancestorsOf, inlineString, limitFor, summarize, visibleLines, type TreeNode,
} from './json-tree';

const MAX_INDENT_DEPTH = 8;

function indent(depth: number): CSSProperties {
    return { paddingInlineStart: `calc(var(--tt-ri-indent) * ${Math.min(depth, MAX_INDENT_DEPTH)})` };
}

function copyText(value: unknown): string {
    return typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? '';
}

function rowsOf(tree: HTMLElement | null): HTMLElement[] {
    return [...(tree?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? [])];
}

/**
 * A JSON value as a tree that starts closed. It knows nothing about wire
 * formats: `summaries` says what a closed node is where the caller knows.
 */
export function JsonTree({ value, label, summaries, open, reveal = null, needle, keep = null, tr, onCopy }: {
    value: unknown;
    label: string;
    summaries?: ReadonlyMap<string, string>;
    /** Containers open at first. */
    open?: readonly string[];
    /** A node brought into view at first. */
    reveal?: string | null;
    needle: string;
    /** The nodes a search kept; all others are left out. */
    keep?: ReadonlySet<string> | null;
    tr: Translate;
    onCopy: (text: string) => void;
}) {
    const [expanded, setExpanded] = useState<ReadonlySet<string>>(
        () => new Set([...(open ?? []), ...(reveal === null ? [] : ancestorsOf(reveal))]),
    );
    const [limits, setLimits] = useState<ReadonlyMap<string, number>>(
        () => new Map(reveal === null ? [] : [...ancestorsOf(reveal), reveal].flatMap(pointer => {
            const limit = limitFor(pointer);
            return limit ? [limit] : [];
        })),
    );
    const [full, setFull] = useState<ReadonlySet<string>>(() => new Set());
    const [active, setActive] = useState(reveal);
    const treeRef = useRef<HTMLDivElement>(null);

    const lines = useMemo(() => visibleLines(value, expanded, limits, keep), [value, expanded, limits, keep]);
    const nodes = lines.flatMap(line => line.type === 'node' ? [line.node] : []);
    const tabStop = nodes.some(node => node.pointer === active) ? active : nodes[0]?.pointer;

    useEffect(() => {
        if (reveal === null) {
            return;
        }
        const row = rowsOf(treeRef.current).find(candidate => candidate.dataset.pointer === reveal);
        row?.scrollIntoView({ block: 'center' });
        row?.focus({ preventScroll: true });
    }, [reveal]);

    const toggle = (pointer: string) => setExpanded(previous => {
        const next = new Set(previous);
        if (!next.delete(pointer)) {
            next.add(pointer);
        }
        return next;
    });

    const handleKey = (event: KeyboardEvent<HTMLDivElement>, node: TreeNode, isOpen: boolean) => {
        if (event.target !== event.currentTarget) {
            return;
        }
        const rows = rowsOf(treeRef.current);
        const at = rows.indexOf(event.currentTarget);
        switch (event.key) {
            case 'ArrowDown': rows[at + 1]?.focus(); break;
            case 'ArrowUp': rows[at - 1]?.focus(); break;
            case 'Home': rows[0]?.focus(); break;
            case 'End': rows.at(-1)?.focus(); break;
            case 'ArrowRight':
                if (node.expandable && !isOpen) {
                    toggle(node.pointer);
                } else {
                    rows[at + 1]?.focus();
                }
                break;
            case 'ArrowLeft':
                if (isOpen) {
                    toggle(node.pointer);
                } else {
                    rows.find(row => row.dataset.pointer === node.parent)?.focus();
                }
                break;
            case 'Enter':
            case ' ':
                if (node.expandable) {
                    toggle(node.pointer);
                }
                break;
            default: return;
        }
        event.preventDefault();
    };

    const items = (count: number) => fill(tr('{count} items'), { count: formatCount(count) });
    const chars = (count: number) => fill(tr('{count} chars'), { count: formatCount(count) });

    return (
        <div ref={treeRef} role="tree" aria-label={label} className="tt-ri-tree">
            {lines.map(line => {
                if (line.type === 'more') {
                    return (
                        <button
                            key={`more:${line.pointer}`}
                            type="button"
                            className="tt-ri-link tt-ri-tree-more"
                            style={indent(line.depth)}
                            onClick={() => setLimits(previous => new Map(previous).set(
                                line.pointer,
                                (previous.get(line.pointer) ?? PAGE) + PAGE,
                            ))}
                        >
                            {fill(tr('Show {count} more'), { count: formatCount(line.rest) })}
                        </button>
                    );
                }
                const { node, expanded: isOpen } = line;
                const text = typeof node.value === 'string' ? node.value : null;
                return (
                    <div key={node.pointer} role="none">
                        <div
                            role="treeitem"
                            aria-level={node.depth + 1}
                            aria-expanded={node.expandable ? isOpen : undefined}
                            aria-selected={false}
                            tabIndex={node.pointer === tabStop ? 0 : -1}
                            data-pointer={node.pointer}
                            className={`tt-ri-node${node.expandable ? ' tt-ri-node-toggle' : ''}`}
                            style={indent(node.depth)}
                            onClick={() => node.expandable && toggle(node.pointer)}
                            onFocus={() => setActive(node.pointer)}
                            onKeyDown={event => handleKey(event, node, isOpen)}
                        >
                            <span className="tt-ri-caret" aria-hidden="true">{node.expandable ? (isOpen ? '▾' : '▸') : ''}</span>
                            {node.label !== null && (
                                <span className="tt-ri-key"><Highlight text={node.label} needle={needle} /></span>
                            )}
                            {text === null ? (
                                node.kind === 'object' || node.kind === 'array' ? (
                                    <span className="tt-ri-dim tt-ri-summary">
                                        {isOpen
                                            ? items(node.size)
                                            : node.size === 0
                                                ? (node.kind === 'array' ? '[]' : '{}')
                                                : summaries?.get(node.pointer) ?? summarize(node.value, items, node.siblings)}
                                    </span>
                                ) : <span className="tt-ri-literal">{JSON.stringify(node.value ?? null)}</span>
                            ) : node.expandable ? (
                                <>
                                    {!isOpen && <span className="tt-ri-summary">{inlineString(text)}</span>}
                                    <span className="tt-ri-dim">{chars(node.size)}</span>
                                </>
                            ) : <span className="tt-ri-string">&quot;<Highlight text={text} needle={needle} />&quot;</span>}
                            {isOpen && (
                                <button
                                    type="button"
                                    className="tt-ri-link tt-ri-node-copy"
                                    onClick={event => {
                                        event.stopPropagation();
                                        onCopy(copyText(node.value));
                                    }}
                                >
                                    {tr('Copy')}
                                </button>
                            )}
                        </div>
                        {isOpen && text !== null && (
                            <div className="tt-ri-tree-text" style={indent(node.depth + 1)}>
                                <TextBlock
                                    text={text}
                                    needle={needle}
                                    full={full.has(node.pointer)}
                                    onShowFull={() => setFull(previous => new Set(previous).add(node.pointer))}
                                    tr={tr}
                                />
                            </div>
                        )}
                    </div>
                );
            })}
        </div>
    );
}
