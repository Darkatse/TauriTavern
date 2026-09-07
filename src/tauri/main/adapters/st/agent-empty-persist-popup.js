// @ts-check

import { translateSillyTavern as tr } from './sillytavern-i18n.js';

export async function confirmEmptyAgentPersist() {
    const { Popup, POPUP_RESULT } = /** @type {any} */ (window).SillyTavern.getContext();
    const result = await Popup.show.confirm(
        tr('tauritavern_agent_persist_missing_title', 'Previous Agent memory is unavailable'),
        tr('tauritavern_agent_persist_missing_body', 'The saved persist state for this chat could not be found. Start this run with empty persist? Chat messages remain available, but this run will not inherit previous Agent memory.'),
        {
            okButton: tr('tauritavern_agent_persist_missing_continue', 'Start with empty persist'),
            cancelButton: tr('tauritavern_agent_persist_missing_cancel', 'Cancel'),
        },
    );
    return result === POPUP_RESULT.AFFIRMATIVE;
}
