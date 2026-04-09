// @ts-check

export const FRONTEND_LOG_RETENTION_LIMITS = Object.freeze({
    debug: 400,
    info: 300,
    warnError: 100,
});

/** @typedef {'debug' | 'info' | 'warn' | 'error'} FrontendLogLevel */

/**
 * @template {{ level: FrontendLogLevel }} T
 * @param {T[]} entries
 */
export function trimFrontendLogEntriesInPlace(entries) {
    let debugCount = 0;
    let infoCount = 0;
    let warnErrorCount = 0;

    /** @type {T[]} */
    const keptReversed = [];

    for (let i = entries.length - 1; i >= 0; i -= 1) {
        const entry = /** @type {T} */ (entries[i]);

        switch (entry.level) {
            case 'debug':
                if (debugCount >= FRONTEND_LOG_RETENTION_LIMITS.debug) break;
                debugCount += 1;
                keptReversed.push(entry);
                break;
            case 'info':
                if (infoCount >= FRONTEND_LOG_RETENTION_LIMITS.info) break;
                infoCount += 1;
                keptReversed.push(entry);
                break;
            case 'warn':
            case 'error':
                if (warnErrorCount >= FRONTEND_LOG_RETENTION_LIMITS.warnError) break;
                warnErrorCount += 1;
                keptReversed.push(entry);
                break;
            default:
                throw new Error(`Unexpected frontend log level: ${/** @type {any} */ (entry).level}`);
        }
    }

    keptReversed.reverse();
    entries.length = 0;
    entries.push(...keptReversed);
}
