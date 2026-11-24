export function formatClock(ms: number): string {
    return new Date(ms).toLocaleTimeString();
}

export function formatDuration(ms: number): string {
    return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
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

/** Fills the `{name}` slots of a translated string. */
export function fill(text: string, values: Record<string, string | number>): string {
    return text.replace(/\{(\w+)\}/gu, (slot, name: string) => name in values ? String(values[name]) : slot);
}
