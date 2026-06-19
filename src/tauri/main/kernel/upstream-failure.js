// @ts-check

/**
 * @typedef {{ code: string; category: string; endpoint: string | null; messageKey: string }} UpstreamFailureDetails
 */

/** @type {Readonly<Record<string, string>>} */
export const UPSTREAM_FAILURE_FALLBACKS = Object.freeze({
    'network.timeout': 'The request timed out before the target service responded.',
    'network.connect_failed': 'Could not connect to the target service. Your network, VPN, proxy, or endpoint address may be unavailable.',
    'network.proxy_failed': 'Could not connect through the configured proxy.',
    'network.dns_failed': 'Could not find the target service address.',
    'network.tls_failed': 'Could not establish a secure connection.',
    'network.body_interrupted': 'The response was interrupted while it was being read.',
    'network.request_failed': 'Network request failed.',
});

/**
 * @param {unknown} value
 * @returns {UpstreamFailureDetails | null}
 */
export function asUpstreamFailureDetails(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value) || value instanceof Error) {
        return null;
    }

    const record = /** @type {Record<string, unknown>} */ (value);
    const code = typeof record.code === 'string' ? record.code.trim() : '';
    const category = typeof record.category === 'string' ? record.category.trim() : '';
    const messageKey = typeof record.message_key === 'string'
        ? record.message_key.trim()
        : (typeof record.messageKey === 'string' ? record.messageKey.trim() : '');

    if (!code || !category || !messageKey) {
        return null;
    }

    const endpoint = typeof record.endpoint === 'string' && record.endpoint.trim()
        ? record.endpoint.trim()
        : null;

    return { code, category, endpoint, messageKey };
}

/**
 * @param {unknown} value
 * @param {number} depth
 * @returns {UpstreamFailureDetails | null}
 */
export function findUpstreamFailureDetails(value, depth = 0) {
    if (depth > 4 || value === null || value === undefined) {
        return null;
    }

    const direct = asUpstreamFailureDetails(value);
    if (direct) {
        return direct;
    }

    if (value instanceof Error) {
        const error = /** @type {Record<string, unknown>} */ (/** @type {unknown} */ (value));
        for (const key of ['details', 'data', 'error']) {
            if (Object.prototype.hasOwnProperty.call(error, key)) {
                const nested = findUpstreamFailureDetails(error[key], depth + 1);
                if (nested) {
                    return nested;
                }
            }
        }

        return findUpstreamFailureDetails(error.cause, depth + 1);
    }

    if (Array.isArray(value)) {
        for (const item of value) {
            const nested = findUpstreamFailureDetails(item, depth + 1);
            if (nested) {
                return nested;
            }
        }
        return null;
    }

    if (typeof value !== 'object') {
        return null;
    }

    const record = /** @type {Record<string, unknown>} */ (value);
    const keys = Object.keys(record);
    if (keys.length === 1 && keys[0] === 'UpstreamFailure') {
        return findUpstreamFailureDetails(record.UpstreamFailure, depth + 1);
    }

    for (const key of ['details', 'error', 'cause', 'data']) {
        if (Object.prototype.hasOwnProperty.call(record, key)) {
            const nested = findUpstreamFailureDetails(record[key], depth + 1);
            if (nested) {
                return nested;
            }
        }
    }

    return null;
}

/**
 * @param {UpstreamFailureDetails | null} details
 */
export function upstreamFailureFallbackText(details) {
    if (!details) {
        return '';
    }

    const message = UPSTREAM_FAILURE_FALLBACKS[details.code] || 'Upstream request failed.';
    return details.endpoint ? `${message} (${details.endpoint})` : message;
}
