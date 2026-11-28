// @ts-check

import { getCommandErrorMessage } from '../../../../scripts/util/command-error-utils.js';
import { translateSillyTavern } from '../../adapters/st/sillytavern-i18n.js';
import {
    asUpstreamFailureDetails,
    findUpstreamFailureDetails,
    upstreamFailureMessage,
} from '../../kernel/upstream-failure.js';

export { asUpstreamFailureDetails };

/**
 * @param {unknown} error
 */
export function getUpstreamFailureDetails(error) {
    return findUpstreamFailureDetails(error);
}

/** @returns {string} */
export function translateApiErrorLabel() {
    return `[${translateSillyTavern('API Error', 'API Error')}]`;
}

/**
 * @param {string} message
 * @returns {string}
 */
export function formatAssistantErrorText(message) {
    return `${translateApiErrorLabel()}\n${message}`;
}

/**
 * @param {unknown} error
 * @param {string} [fallbackMessage]
 * @returns {string}
 */
export function getUserFacingErrorMessage(error, fallbackMessage = 'Generation request failed') {
    const details = getUpstreamFailureDetails(error);
    if (!details) {
        return getCommandErrorMessage(error) || fallbackMessage;
    }

    const lines = [upstreamFailureMessage(details, translateSillyTavern)];

    if (details.category === 'network') {
        lines.push(
            '',
            translateSillyTavern(
                'tauritavern.error.network.action',
                'Check your network, VPN, proxy, or custom endpoint address, then try again.',
            ),
        );
    }

    if (details.endpoint) {
        lines.push(
            '',
            `${translateSillyTavern('tauritavern.error.network.endpoint_label', 'Endpoint')}: ${details.endpoint}`,
        );
    }

    return lines.join('\n');
}

/**
 * @param {unknown} error
 * @returns {{ error: Record<string, unknown> }}
 */
export function buildLegacyErrorPayload(error) {
    const details = getUpstreamFailureDetails(error);
    /** @type {Record<string, unknown>} */
    const payload = {
        message: getUserFacingErrorMessage(error),
    };

    if (details) {
        payload.code = details.code;
        payload.category = details.category;
        payload.message_key = details.messageKey;
        if (details.endpoint) {
            payload.endpoint = details.endpoint;
        }
    }

    return { error: payload };
}
