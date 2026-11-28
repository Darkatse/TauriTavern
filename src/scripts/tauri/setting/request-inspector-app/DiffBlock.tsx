import { type ReactNode, useMemo } from 'react';

import type { Translate } from './RequestInspectorContract';
import { Highlight } from './TextBlock';
import { fill, formatCount } from './format';
import { type DiffLine, type TextRange, changedRanges } from './text-diff';

const CONTEXT_LINES = 2;
const LONG_LINE_CHARS = 320;
const CONTEXT_CHARS = 60;
const LONG_LINE_CHANGES = 3;

const SIGNS = { same: '', added: '+', removed: '−' } as const;

function lineRanges(lines: DiffLine[]): Map<number, TextRange[]> {
    const ranges = new Map<number, TextRange[]>();
    for (let start = 0; start < lines.length; start++) {
        if (lines[start]?.op !== 'removed') {
            continue;
        }
        let added = start;
        while (lines[added]?.op === 'removed') {
            added += 1;
        }
        for (let offset = 0; start + offset < added && lines[added + offset]?.op === 'added'; offset++) {
            const pair = changedRanges(lines[start + offset]?.text ?? '', lines[added + offset]?.text ?? '');
            if (pair) {
                ranges.set(start + offset, pair.before);
                ranges.set(added + offset, pair.after);
            }
        }
        start = added;
    }
    return ranges;
}

function nearChanges(lines: DiffLine[]): boolean[] {
    const near = lines.map(() => false);
    lines.forEach((line, index) => {
        if (line.op !== 'same') {
            for (let at = Math.max(0, index - CONTEXT_LINES); at <= Math.min(lines.length - 1, index + CONTEXT_LINES); at++) {
                near[at] = true;
            }
        }
    });
    const aloneBetweenShown = (index: number) => near[index - 1] === true && near[index + 1] === true;
    return near.map((shown, index) => shown || aloneBetweenShown(index));
}

function windowsOf(text: string, ranges: TextRange[]): TextRange[] {
    if (text.length <= LONG_LINE_CHARS) {
        return [{ start: 0, end: text.length }];
    }
    if (ranges.length === 0) {
        return [{ start: 0, end: LONG_LINE_CHARS }];
    }
    const windows: TextRange[] = [];
    for (const range of ranges.slice(0, LONG_LINE_CHANGES)) {
        const start = Math.max(0, range.start - CONTEXT_CHARS);
        const end = Math.min(text.length, Math.min(range.end, range.start + LONG_LINE_CHARS) + CONTEXT_CHARS);
        const last = windows.at(-1);
        if (last && start <= last.end) {
            last.end = Math.max(last.end, end);
        } else {
            windows.push({ start, end });
        }
    }
    return windows;
}

function Line({ line, ranges, cut, needle }: { line: DiffLine; ranges: TextRange[]; cut: boolean; needle: string }) {
    const { text, op } = line;
    const Changed = op === 'removed' ? 'del' : 'ins';
    const windows = cut ? windowsOf(text, ranges) : [{ start: 0, end: text.length }];
    const pieces: ReactNode[] = [];
    windows.forEach((window, index) => {
        if (window.start > (windows[index - 1]?.end ?? 0)) {
            pieces.push(<span key={`gap-${index}`} className="tt-ri-diff-cut">⋯</span>);
        }
        let from = window.start;
        for (const range of ranges) {
            const start = Math.max(range.start, from);
            const end = Math.min(range.end, window.end);
            if (start < end) {
                pieces.push(
                    <Highlight key={`text-${start}`} text={text.slice(from, start)} needle={needle} />,
                    <Changed key={`changed-${start}`}><Highlight text={text.slice(start, end)} needle={needle} /></Changed>,
                );
                from = end;
            }
        }
        pieces.push(<Highlight key={`rest-${index}`} text={text.slice(from, window.end)} needle={needle} />);
    });
    if ((windows.at(-1)?.end ?? 0) < text.length) {
        pieces.push(<span key="tail" className="tt-ri-diff-cut">⋯</span>);
    }
    return (
        <div className="tt-ri-diff-line" data-op={op}>
            <span className="tt-ri-sign">{SIGNS[op]}</span>
            <span className="tt-ri-diff-text">{pieces}</span>
        </div>
    );
}

export function DiffBlock({ lines, needle, full, onShowFull, tr }: {
    lines: DiffLine[];
    needle: string;
    full: boolean;
    onShowFull: () => void;
    tr: Translate;
}) {
    const ranges = useMemo(() => lineRanges(lines), [lines]);
    const shown = useMemo(() => nearChanges(lines), [lines]);
    const cut = !full && (shown.includes(false) || lines.some(line => line.text.length > LONG_LINE_CHARS));

    const rows: ReactNode[] = [];
    let skipped = 0;
    lines.forEach((line, index) => {
        if (!full && shown[index] === false) {
            skipped += 1;
            return;
        }
        if (skipped > 0) {
            rows.push(
                <div key={`skip-${index}`} className="tt-ri-diff-skip">
                    {fill(tr('{count} lines unchanged'), { count: formatCount(skipped) })}
                </div>,
            );
            skipped = 0;
        }
        rows.push(<Line key={index} line={line} ranges={ranges.get(index) ?? []} cut={!full} needle={needle} />);
    });
    if (skipped > 0) {
        rows.push(
            <div key="skip-end" className="tt-ri-diff-skip">
                {fill(tr('{count} lines unchanged'), { count: formatCount(skipped) })}
            </div>,
        );
    }

    return (
        <>
            <div className="tt-ri-text tt-ri-diff">{rows}</div>
            {cut && (
                <button type="button" className="tt-ri-link" onClick={onShowFull}>
                    {fill(tr('Show all ({count} lines)'), { count: formatCount(lines.length) })}
                </button>
            )}
        </>
    );
}
