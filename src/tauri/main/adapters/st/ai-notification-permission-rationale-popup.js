// @ts-check

import { translateSillyTavern } from './sillytavern-i18n.js';

function buildPopupContent() {
    const container = document.createElement('div');
    container.className = 'flex-container flexFlowColumn';
    container.style.gap = '10px';

    const title = document.createElement('b');
    title.textContent = translateSillyTavern(
        'tauritavern_notification_permission_rationale_title',
        'Enable notifications?',
    );

    const body = document.createElement('div');
    body.textContent = translateSillyTavern(
        'tauritavern_notification_permission_rationale_body',
        'When the AI reply is ready, we can send you a system notification. Enable it?',
    );
    body.style.whiteSpace = 'pre-wrap';

    container.append(title, body);
    return container;
}

export async function confirmAiNotificationPermissionRationale() {
    const { callGenericPopup, POPUP_RESULT, POPUP_TYPE } = await import(
        '../../../../scripts/popup.js'
    );

    const okButton = translateSillyTavern(
        'tauritavern_notification_permission_rationale_allow',
        'Enable',
    );
    const cancelButton = translateSillyTavern(
        'tauritavern_notification_permission_rationale_cancel',
        'Not now',
    );

    const result = await callGenericPopup(buildPopupContent(), POPUP_TYPE.CONFIRM, '', {
        okButton,
        cancelButton,
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    return result === POPUP_RESULT.AFFIRMATIVE;
}

