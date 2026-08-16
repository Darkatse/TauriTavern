const RRF_OFFSET = 60;

/**
 * Combines ranked result lists without assuming their scores are comparable.
 * Duplicate items within one list contribute only once.
 * @template T
 * @param {T[][]} rankings Ranked result lists, best item first
 * @param {number} limit Maximum number of fused results
 * @returns {T[]} Fused results, best item first
 */
export function reciprocalRankFusion(rankings, limit) {
    const scores = new Map();

    for (const ranking of rankings) {
        [...new Set(ranking)].forEach((item, rank) => {
            scores.set(item, (scores.get(item) ?? 0) + 1 / (RRF_OFFSET + rank + 1));
        });
    }

    return [...scores.entries()]
        .sort((left, right) => right[1] - left[1])
        .slice(0, limit)
        .map(([item]) => item);
}
