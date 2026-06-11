import { callGenericPopup, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';

export async function showErrorPopup(error) {
    const message = error?.message ? String(error.message) : String(error);
    await callGenericPopup(translate(message), POPUP_TYPE.TEXT, '', {
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
