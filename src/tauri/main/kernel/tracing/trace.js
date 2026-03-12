// @ts-check

export const DEFAULT_TRACE_HEADER = 'x-tauritavern-trace-id';

/**
 * @param {string} prefix
 * @returns {() => string}
 */
export function createTraceIdFactory(prefix = 'tt') {
    const normalizedPrefix = String(prefix || 'tt').trim() || 'tt';
    let sequence = 0;

    return () => {
        sequence = (sequence + 1) >>> 0;
        return `${normalizedPrefix}-${Date.now().toString(36)}-${sequence.toString(36)}`;
    };
}

