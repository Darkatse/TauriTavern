import { expect, test } from '@rstest/core';

import { changedRanges, diffLines, diffSequences } from './text-diff';

function longestCommonSubsequence(a: string[], b: string[]): number {
    const table = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
    for (let i = 1; i <= a.length; i++) {
        for (let j = 1; j <= b.length; j++) {
            table[i]![j] = a[i - 1] === b[j - 1]
                ? table[i - 1]![j - 1]! + 1
                : Math.max(table[i - 1]![j]!, table[i]![j - 1]!);
        }
    }
    return table[a.length]![b.length]!;
}

test('a sequence diff replays both sides, keeps as much as can be kept, and puts what was removed first', () => {
    let seed = 7;
    const random = (below: number) => {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        return Math.floor(seed / 2147483648 * below);
    };
    const sequence = () => Array.from({ length: random(14) }, () => 'abcd'.charAt(random(4)));

    for (let round = 0; round < 300; round++) {
        const before = sequence();
        const after = sequence();
        const steps = diffSequences(before, after, item => item);
        if (steps === null) throw new Error('short sequences are always lined up');

        expect(steps.flatMap(step => step.op === 'added' ? [] : [step.before])).toEqual(before);
        expect(steps.flatMap(step => step.op === 'removed' ? [] : [step.after])).toEqual(after);
        expect(steps.filter(step => step.op === 'same').length).toBe(longestCommonSubsequence(before, after));
        steps.forEach((step, index) => {
            if (step.op === 'removed') expect(steps[index - 1]?.op).not.toBe('added');
        });
    }
});

test('texts too different to line up come out as one replaced by the other', () => {
    const text = (prefix: string) => Array.from({ length: 2500 }, (_, index) => `${prefix}${index}`).join('\n');
    const lines = diffLines(text('a'), text('b'));
    expect(lines.length).toBe(5000);
    expect(lines.slice(0, 2500).every(line => line.op === 'removed')).toBe(true);
    expect(lines.slice(2500).every(line => line.op === 'added')).toBe(true);
});

test('a changed line marks the words that differ; lines with little in common are two lines', () => {
    const before = 'Keep a restrained tone and never decide for the user.';
    const after = 'Keep a cold tone and never speak for the user.';
    const ranges = changedRanges(before, after);
    if (ranges === null) throw new Error('the lines share most of their words');
    expect(ranges.before.map(({ start, end }) => before.slice(start, end))).toEqual(['restrained', 'decide']);
    expect(ranges.after.map(({ start, end }) => after.slice(start, end))).toEqual(['cold', 'speak']);

    expect(changedRanges('Stay calm.', 'Be brief.')).toBeNull();
});
