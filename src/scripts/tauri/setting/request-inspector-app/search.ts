import { leadNear } from './format';

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

/** The line holding the first match, cut to a preview that starts near the match. */
export function matchingLine(text: string, needle: string): string | null {
    const at = needle ? haystack(text).indexOf(needle) : -1;
    if (at < 0) {
        return null;
    }
    const lineStart = text.lastIndexOf('\n', at - 1) + 1;
    const lineEnd = text.indexOf('\n', at);
    return leadNear(text.slice(lineStart, lineEnd < 0 ? undefined : lineEnd), at - lineStart).lead;
}
