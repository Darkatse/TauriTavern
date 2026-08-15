const REQUIRED_TAG_LITERAL_REGEX = /^(?:\^)?(?:\.\*\??)?(<(?:\\\/)?[A-Za-z0-9_:\u0080-\uFFFF-]+)(?=>|\[|\\\/>)/;

/**
 * Extracts a case-sensitive tag prefix that every match must contain.
 * @param {RegExp} regex The compiled regular expression
 * @returns {string|null} The required literal, or null when it cannot be proven
 */
export function getRequiredTagLiteral(regex) {
    if (regex.ignoreCase || regex.source.includes('|')) {
        return null;
    }

    return regex.source.match(REQUIRED_TAG_LITERAL_REGEX)?.[1].replace('\\/', '/') ?? null;
}
