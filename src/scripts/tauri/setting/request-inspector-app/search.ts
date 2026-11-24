/**
 * Plain-text search shared by both views. A query is matched without regard to
 * case; `needle` is the query already lowercased.
 */

function haystack(text: string): string {
    const lower = text.toLowerCase();
    // Lowercasing can change the length; offsets then belong to the original.
    return lower.length === text.length ? lower : text;
}

export function matchOffsets(text: string, needle: string): number[] {
    if (!needle) {
        return [];
    }
    const source = haystack(text);
    const offsets: number[] = [];
    for (let at = source.indexOf(needle); at >= 0; at = source.indexOf(needle, at + needle.length)) {
        offsets.push(at);
    }
    return offsets;
}

export function countMatches(text: string, needle: string): number {
    return matchOffsets(text, needle).length;
}

const LEAD_CONTEXT = 24;

/** The line holding the first match, cut so the match stays near its start. */
export function matchingLine(text: string, needle: string): string | null {
    const at = needle ? haystack(text).indexOf(needle) : -1;
    if (at < 0) {
        return null;
    }
    const lineStart = text.lastIndexOf('\n', at - 1) + 1;
    const lineEnd = text.indexOf('\n', at);
    const start = at - lineStart > LEAD_CONTEXT ? at - LEAD_CONTEXT : lineStart;
    const line = text.slice(start, lineEnd < 0 ? undefined : lineEnd).trim();
    return start > lineStart ? `…${line}` : line;
}
