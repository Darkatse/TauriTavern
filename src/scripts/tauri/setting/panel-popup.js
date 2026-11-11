import { Popup } from '../../popup.js';

export const TAURITAVERN_PANEL_POPUP_CLASS = 'tt-tauritavern-panel-popup';

export function createTauriTavernPanelPopup(content, type, inputValue = '', options = {}) {
    const popup = new Popup(content, type, inputValue, options);
    popup.dlg.classList.add(TAURITAVERN_PANEL_POPUP_CLASS);
    return popup;
}

export function callTauriTavernPanelPopup(content, type, inputValue = '', options = {}) {
    return createTauriTavernPanelPopup(content, type, inputValue, options).show();
}
