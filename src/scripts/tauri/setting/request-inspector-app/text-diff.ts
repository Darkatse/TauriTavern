export type Step<T> =
    | { op: 'same'; before: T; after: T }
    | { op: 'removed'; before: T }
    | { op: 'added'; after: T };

export type DiffLine = { op: Step<string>['op']; text: string };

export type TextRange = { start: number; end: number };

/** The search keeps a table of this size squared. */
const MAX_DIFFERENCES = 1500;
const MAX_COMPARISONS = 20_000_000;

const KEEP = 0;
const REMOVE = 1;
const ADD = 2;

/** The index is one the search has shown to be inside the array; the fallback is for the type alone. */
function at(values: Int32Array, index: number): number {
    return values[index] ?? 0;
}

/** Myers' O(ND) shortest edit script from `a` to `b`, one code per step; `null` past the limit. */
function editScript(a: Int32Array, b: Int32Array): number[] | null {
    const limit = Math.min(a.length + b.length, MAX_DIFFERENCES, Math.floor(MAX_COMPARISONS / (a.length + b.length + 1)));
    // reach[k + origin] is how far along `a` the diagonal k (x - y) has got.
    const origin = limit + 1;
    const reach = new Int32Array(2 * limit + 3);
    // rounds[d] is `reach` over the diagonals -d to d, after d differences.
    const rounds: Int32Array[] = [];

    for (let differences = 0; differences <= limit; differences++) {
        for (let k = -differences; k <= differences; k += 2) {
            const down = k === -differences || (k !== differences && at(reach, origin + k - 1) < at(reach, origin + k + 1));
            let x = down ? at(reach, origin + k + 1) : at(reach, origin + k - 1) + 1;
            let y = x - k;
            while (x < a.length && y < b.length && a[x] === b[y]) {
                x += 1;
                y += 1;
            }
            reach[origin + k] = x;
            if (x >= a.length && y >= b.length) {
                return walkBack(rounds, differences, a.length, b.length);
            }
        }
        rounds.push(reach.slice(origin - differences, origin + differences + 1));
    }
    return null;
}

function walkBack(rounds: Int32Array[], differences: number, endX: number, endY: number): number[] {
    const script: number[] = [];
    let x = endX;
    let y = endY;
    for (let round = differences; round > 0; round--) {
        const earlier = rounds[round - 1] ?? new Int32Array();
        const reached = (k: number) => at(earlier, k + round - 1);
        const k = x - y;
        const down = k === -round || (k !== round && reached(k - 1) < reached(k + 1));
        const fromK = down ? k + 1 : k - 1;
        const fromX = reached(fromK);
        const fromY = fromX - fromK;
        while (x > fromX && y > fromY) {
            script.push(KEEP);
            x -= 1;
            y -= 1;
        }
        script.push(down ? ADD : REMOVE);
        x = fromX;
        y = fromY;
    }
    for (; x > 0; x--) {
        script.push(KEEP);
    }
    return script.reverse();
}

/**
 * Where the two differ, what was removed comes before what was added. `null`
 * when they differ in more places than the search explores.
 */
export function diffSequences<T>(
    before: readonly T[],
    after: readonly T[],
    key: (item: T) => string,
): Step<T>[] | null {
    const ids = new Map<string, number>();
    const encode = (items: readonly T[]) => Int32Array.from(items, (item) => {
        const itemKey = key(item);
        const id = ids.get(itemKey) ?? ids.size;
        ids.set(itemKey, id);
        return id;
    });
    const a = encode(before);
    const b = encode(after);

    const shortest = Math.min(a.length, b.length);
    let head = 0;
    while (head < shortest && a[head] === b[head]) {
        head += 1;
    }
    let tail = 0;
    while (tail < shortest - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) {
        tail += 1;
    }
    const script = editScript(a.subarray(head, a.length - tail), b.subarray(head, b.length - tail));
    if (script === null) {
        return null;
    }

    const steps: Step<T>[] = [];
    let added: Step<T>[] = [];
    let x = 0;
    let y = 0;
    const keep = () => {
        steps.push(...added, { op: 'same', before: before[x] as T, after: after[y] as T });
        added = [];
        x += 1;
        y += 1;
    };
    for (let index = 0; index < head; index++) {
        keep();
    }
    for (const code of script) {
        if (code === KEEP) {
            keep();
        } else if (code === REMOVE) {
            steps.push({ op: 'removed', before: before[x] as T });
            x += 1;
        } else {
            added.push({ op: 'added', after: after[y] as T });
            y += 1;
        }
    }
    steps.push(...added);
    added = [];
    for (let index = 0; index < tail; index++) {
        keep();
    }
    return steps;
}

function linesOf(text: string): string[] {
    return text === '' ? [] : text.split('\n');
}

export function diffLines(before: string, after: string): DiffLine[] {
    const removed = linesOf(before);
    const added = linesOf(after);
    const steps = diffSequences(removed, added, line => line);
    if (steps === null) {
        return [
            ...removed.map(text => ({ op: 'removed' as const, text })),
            ...added.map(text => ({ op: 'added' as const, text })),
        ];
    }
    return steps.map(step => ({ op: step.op, text: step.op === 'removed' ? step.before : step.after }));
}

const MAX_WORD_DIFF_CHARS = 2000;
/** Shared characters taken along on each side of what differs, so the words at its edges are whole. */
const WORD_CONTEXT = 32;

let segmenter: Intl.Segmenter | undefined;

function wordsOf(text: string): { text: string; start: number }[] {
    segmenter ??= new Intl.Segmenter(undefined, { granularity: 'word' });
    return Array.from(segmenter.segment(text), ({ segment, index }) => ({ text: segment, start: index }));
}

const isHighSurrogate = (code: number) => code >= 0xd800 && code <= 0xdbff;
const isLowSurrogate = (code: number) => code >= 0xdc00 && code <= 0xdfff;

function joinRanges(ranges: TextRange[], text: string): TextRange[] {
    const joined: TextRange[] = [];
    for (const range of ranges) {
        const last = joined.at(-1);
        if (last && text.slice(last.end, range.start).trim() === '') {
            last.end = range.end;
        } else {
            joined.push({ ...range });
        }
    }
    return joined.map((range) => {
        const words = text.slice(range.start, range.end);
        const start = range.start + words.length - words.trimStart().length;
        return words.trim() === '' ? range : { start, end: start + words.trim().length };
    });
}

/** `null` when the lines share less than half of the longer one: they are two lines, not one that changed. */
export function changedRanges(before: string, after: string): { before: TextRange[]; after: TextRange[] } | null {
    const shortest = Math.min(before.length, after.length);
    let head = 0;
    while (head < shortest && before.charCodeAt(head) === after.charCodeAt(head)) {
        head += 1;
    }
    if (head > 0 && isHighSurrogate(before.charCodeAt(head - 1))) {
        head -= 1;
    }
    let tail = 0;
    while (tail < shortest - head && before.charCodeAt(before.length - 1 - tail) === after.charCodeAt(after.length - 1 - tail)) {
        tail += 1;
    }
    if (tail > 0 && isLowSurrogate(before.charCodeAt(before.length - tail))) {
        tail -= 1;
    }

    const whole = (text: string): TextRange[] => head + tail < text.length ? [{ start: head, end: text.length - tail }] : [];
    let shared = head + tail;
    let ranges = { before: whole(before), after: whole(after) };

    if (Math.max(before.length, after.length) - head - tail <= MAX_WORD_DIFF_CHARS) {
        const from = Math.max(0, head - WORD_CONTEXT);
        const kept = Math.max(0, tail - WORD_CONTEXT);
        const steps = diffSequences(
            wordsOf(before.slice(from, before.length - kept)),
            wordsOf(after.slice(from, after.length - kept)),
            word => word.text,
        );
        if (steps !== null) {
            const range = (word: { text: string; start: number }): TextRange => ({
                start: from + word.start,
                end: from + word.start + word.text.length,
            });
            shared = from + kept;
            ranges = { before: [], after: [] };
            for (const step of steps) {
                if (step.op === 'same') {
                    shared += step.after.text.length;
                } else if (step.op === 'removed') {
                    ranges.before.push(range(step.before));
                } else {
                    ranges.after.push(range(step.after));
                }
            }
            ranges = { before: joinRanges(ranges.before, before), after: joinRanges(ranges.after, after) };
        }
    }

    return shared * 2 < Math.max(before.length, after.length) ? null : ranges;
}
