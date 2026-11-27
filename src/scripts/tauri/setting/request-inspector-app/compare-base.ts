import type { EarlierPreview, LogIndexEntry, Selection, Translate } from './RequestInspectorContract';
import { formatClock, logLabel } from './format';

export type BaseRef = { kind: 'log'; id: number } | { kind: 'earlier' };

export type BaseOption = { ref: BaseRef; key: string; timestampMs: number; label: string };

export function baseKey(ref: BaseRef): string {
    return ref.kind === 'log' ? String(ref.id) : 'earlier';
}

/** Newest first, which `nearestBase` relies on. */
export function baseOptions(
    selection: Selection,
    entries: LogIndexEntry[],
    earlier: EarlierPreview | null,
    tr: Translate,
): BaseOption[] {
    const options: BaseOption[] = entries
        .filter(entry => selection.kind === 'preview' || entry.id !== selection.id)
        .map(entry => ({
            ref: { kind: 'log', id: entry.id },
            key: String(entry.id),
            timestampMs: entry.timestampMs,
            label: logLabel(entry),
        }));
    if (selection.kind === 'preview' && earlier) {
        options.push({
            ref: { kind: 'earlier' },
            key: 'earlier',
            timestampMs: earlier.timestampMs,
            label: `${tr('Earlier preview')} · ${formatClock(earlier.timestampMs)}`,
        });
    }
    return options.sort((a, b) => b.timestampMs - a.timestampMs);
}

export function nearestBase(options: BaseOption[], timestampMs: number): BaseOption | undefined {
    return options.find(option => option.timestampMs <= timestampMs) ?? options.at(-1);
}
