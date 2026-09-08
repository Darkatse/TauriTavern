import { callGenericPopup, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';
import { toUserFacingErrorText } from '../../../util/user-facing-error.js';

export async function showErrorPopup(error) {
    await callGenericPopup(toUserFacingErrorText(error), POPUP_TYPE.TEXT, '', {
        okButton: translate('OK'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
}

export async function runTaskOrPopup(task) {
    try {
        return await task();
    } catch (error) {
        await showErrorPopup(error);
        return undefined;
    }
}

export function runOrPopup(task) {
    void runTaskOrPopup(task);
}
