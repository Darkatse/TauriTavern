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

/** The first line with content, without surrounding whitespace. */
export function firstLine(text: string): string {
    const start = text.search(/\S/u);
    if (start < 0) {
        return '';
    }
    const end = text.indexOf('\n', start);
    return (end < 0 ? text.slice(start) : text.slice(start, end)).trimEnd();
}

/** Fills the `{name}` slots of a translated string. */
export function fill(text: string, values: Record<string, string | number>): string {
    return text.replace(/\{(\w+)\}/gu, (slot, name: string) => name in values ? String(values[name]) : slot);
}
