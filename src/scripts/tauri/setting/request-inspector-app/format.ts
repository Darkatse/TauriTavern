import type { LogIndexEntry } from './RequestInspectorContract';

export function formatClock(ms: number): string {
    return new Date(ms).toLocaleTimeString();
}

export function formatDuration(ms: number): string {
    return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

export function logLabel(entry: LogIndexEntry): string {
    return `${entry.ok ? '✓' : '✗'} ${formatClock(entry.timestampMs)} · ${entry.model ?? entry.source} · ${formatDuration(entry.durationMs)}`;
}

export function formatCount(value: number): string {
    return value.toLocaleString();
}

/** Compact size for a row; the exact figure is in the summary line. */
export function formatTokens(value: number): string {
    return value < 1000 ? String(value) : `${(value / 1000).toFixed(1)}k`;
}

/** Characters a one-line preview keeps; a browser lays out all of a line, however little of it shows. */
export const LEAD_LENGTH = 160;

export function clip(text: string, length: number): string {
    return text.length > length ? `${text.slice(0, length)}…` : text;
}

/** The first line with content, without surrounding whitespace, as a one-line preview. */
export function firstLine(text: string): string {
    const start = text.search(/\S/u);
    if (start < 0) {
        return '';
    }
    const end = text.indexOf('\n', start);
    return clip((end < 0 ? text.slice(start) : text.slice(start, end)).trimEnd(), LEAD_LENGTH);
}

const LEAD_CONTEXT = 24;

/** A one-line preview of `line` that starts near `offset`, and where that offset is in the preview. */
export function leadNear(line: string, offset: number): { lead: string; offset: number } {
    const start = offset > LEAD_CONTEXT ? offset - LEAD_CONTEXT : 0;
    const rest = line.slice(start);
    const text = rest.trimStart();
    const lead = clip(text.trimEnd(), LEAD_LENGTH);
    const shown = offset - start - (rest.length - text.length);
    return start > 0 ? { lead: `…${lead}`, offset: shown + 1 } : { lead, offset: shown };
}

export function isScalar(value: unknown): boolean {
    return value === null || typeof value !== 'object';
}

export function prettyJson(value: unknown): string {
    return JSON.stringify(value, null, 2) ?? '';
}

/** Fills the `{name}` slots of a translated string. */
export function fill(text: string, values: Record<string, string | number>): string {
    return text.replace(/\{(\w+)\}/gu, (slot, name: string) => name in values ? String(values[name]) : slot);
}
