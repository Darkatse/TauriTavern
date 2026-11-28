// @ts-check

import { translateSillyTavern } from '../../adapters/st/sillytavern-i18n.js';

const FAILURE_NOTIFICATION_MAX_BODY_LENGTH = 180;
const i18nNotificationKeys = Object.freeze({
    successTitle: 'tauritavern_ai_notification_success_title',
    successBody: 'tauritavern_ai_notification_success_body',
    failureTitle: 'tauritavern_ai_notification_failure_title',
    failureBody: 'tauritavern_ai_notification_failure_body',
});
const i18nNotificationFallbacks = Object.freeze({
    successTitle: 'AI reply is ready',
    successBody: 'Tap to return to TauriTavern',
    failureTitle: 'AI reply failed',
    failureBody: 'Generation failed. Tap to return to TauriTavern',
});

/** @returns {boolean} */
export function shouldNotifyCompletion() {
    if (document.visibilityState === 'hidden') {
        return true;
    }

    return typeof document.hasFocus === 'function' && !document.hasFocus();
}

export function getGenerationNotificationTexts() {
    return {
        successTitle: translateSillyTavern(i18nNotificationKeys.successTitle, i18nNotificationFallbacks.successTitle),
        successBody: translateSillyTavern(i18nNotificationKeys.successBody, i18nNotificationFallbacks.successBody),
        failureTitle: translateSillyTavern(i18nNotificationKeys.failureTitle, i18nNotificationFallbacks.failureTitle),
        failureBody: translateSillyTavern(i18nNotificationKeys.failureBody, i18nNotificationFallbacks.failureBody),
    };
}

/**
 * @param {string} errorMessage
 * @returns {string}
 */
export function normalizeFailureNotificationBody(errorMessage) {
    const normalized = errorMessage.trim();
    if (normalized.length > FAILURE_NOTIFICATION_MAX_BODY_LENGTH) {
        return `${normalized.slice(0, FAILURE_NOTIFICATION_MAX_BODY_LENGTH - 3)}...`;
    }

    return normalized;
}
