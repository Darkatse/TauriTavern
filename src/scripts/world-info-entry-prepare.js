/**
 * Parses decorators and hashes entries without retaining an intermediate entry array.
 * Hash input and property order match the legacy two-map implementation.
 * @param {object[]} entries
 * @param {(content: string) => [string[], string]} parseDecorators
 * @param {(value: string) => number} getStringHash
 * @returns {object[]}
 */
export function prepareWorldInfoEntries(entries, parseDecorators, getStringHash) {
    return entries.map((entry) => {
        const [decorators, content] = parseDecorators(entry.content || '');
        const preparedEntry = { ...entry, decorators, content };
        preparedEntry.hash = getStringHash(JSON.stringify(preparedEntry));
        return preparedEntry;
    });
}
