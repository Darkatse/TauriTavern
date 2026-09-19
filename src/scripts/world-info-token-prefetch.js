const DEFAULT_WORLD_INFO_TOKEN_BATCH_SIZE = 64;
const LEGACY_WORLD_INFO_MACRO = /<(?:USER|BOT|CHAR|GROUP|CHARIFNOTGROUP)>/i;

export function canPrefetchWorldInfoTokenCount(entry) {
    const content = String(entry.content ?? '');
    return !entry.ignoreBudget
        && (!entry.useProbability || entry.probability === 100)
        && !content.includes('{{')
        && !LEGACY_WORLD_INFO_MACRO.test(content);
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
