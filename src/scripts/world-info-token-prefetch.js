const DEFAULT_WORLD_INFO_TOKEN_BATCH_SIZE = 64;

export function canPrefetchWorldInfoTokenCount(entry) {
    return !entry.ignoreBudget
        && (!entry.useProbability || entry.probability === 100)
        && !String(entry.content ?? '').includes('{')
        && !String(entry.content ?? '').includes('<');
}

export function getWorldInfoTokenPrefetchBatch(entries, startIndex, maxEntries = DEFAULT_WORLD_INFO_TOKEN_BATCH_SIZE) {
    const batchEntries = [];
    const suffixes = [];

    for (let index = startIndex; index < entries.length && batchEntries.length < maxEntries; index++) {
        const entry = entries[index];
        if (!canPrefetchWorldInfoTokenCount(entry)) {
            break;
        }

        batchEntries.push(entry);
        suffixes.push(`${entry.content}\n`);
    }

    return { entries: batchEntries, suffixes };
}
