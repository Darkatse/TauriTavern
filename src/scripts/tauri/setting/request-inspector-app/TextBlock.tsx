import type { ReactNode } from 'react';

import type { Translate } from './RequestInspectorContract';
import { fill, formatCount } from './format';
import type { TextLabel } from './outline';
import { matchOffsets } from './search';

const CLAMP_OVER_LINES = 30;
const CLAMP_LINES = 20;
const CLAMP_OVER_CHARS = 6000;
const CLAMP_CHARS = 4000;

/** The start of a long text, shown until the reader asks for all of it; `null` for a short one. */
export function clampText(text: string): string | null {
    let breaks = 0;
    let cut = text.length;
    for (let at = text.indexOf('\n'); at >= 0 && breaks < CLAMP_OVER_LINES; at = text.indexOf('\n', at + 1)) {
        breaks += 1;
        if (breaks === CLAMP_LINES) {
            cut = at;
        }
    }
    if (breaks < CLAMP_OVER_LINES && text.length <= CLAMP_OVER_CHARS) {
        return null;
    }
    return text.slice(0, Math.min(cut, CLAMP_CHARS));
}

export function Highlight({ text, needle }: { text: string; needle: string }): ReactNode {
    const offsets = matchOffsets(text, needle);
    if (offsets.length === 0) {
        return text;
    }
    const nodes: ReactNode[] = [];
    let from = 0;
    offsets.forEach((at, index) => {
        const end = at + needle.length;
        nodes.push(text.slice(from, at), <mark key={index} className="tt-ri-mark">{text.slice(at, end)}</mark>);
        from = end;
    });
    nodes.push(text.slice(from));
    return nodes;
}

/** Text as it was sent: whitespace kept, long text cut until asked for. */
export function TextBlock({ text, labels = [], needle, full, onShowFull, tr }: {
    text: string;
    labels?: TextLabel[];
    needle: string;
    full: boolean;
    onShowFull: () => void;
    tr: Translate;
}) {
    const head = full ? null : clampText(text);
    const shown = head ?? text;
    const pieces: ReactNode[] = [];
    let from = 0;
    labels.filter(label => label.offset <= shown.length).forEach((label, index) => {
        pieces.push(
            <Highlight key={`text-${index}`} text={shown.slice(from, label.offset)} needle={needle} />,
            <span key={`label-${index}`} className="tt-ri-label">{label.name}</span>,
        );
        from = label.offset;
    });

    return (
        <>
            <div className="tt-ri-text">
                {pieces}
                <Highlight text={shown.slice(from)} needle={needle} />
            </div>
            {head !== null && (
                <button type="button" className="tt-ri-link" onClick={onShowFull}>
                    {fill(tr('Show all ({count} chars)'), { count: formatCount(text.length) })}
                </button>
            )}
        </>
    );
}
