import { Popup } from '../../popup.js';

export const TAURITAVERN_PANEL_POPUP_CLASS = 'tt-tauritavern-panel-popup';

const MOBILE_SURFACE_ATTR = 'data-tt-mobile-surface';
const FULLSCREEN_WINDOW_SURFACE = 'fullscreen-window';

export function createTauriTavernPanelPopup(content, type, inputValue = '', options = {}) {
    const popup = new Popup(content, type, inputValue, options);
    popup.dlg.classList.add(TAURITAVERN_PANEL_POPUP_CLASS);
    popup.dlg.setAttribute(MOBILE_SURFACE_ATTR, FULLSCREEN_WINDOW_SURFACE);
    return popup;
}

export function callTauriTavernPanelPopup(content, type, inputValue = '', options = {}) {
    return createTauriTavernPanelPopup(content, type, inputValue, options).show();
}
