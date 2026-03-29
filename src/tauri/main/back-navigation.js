import { consumeBackNavigationHandlers } from './services/back-navigation/back-handler-stack.js';

const BACK_HANDLER_KEY = '__TAURITAVERN_HANDLE_BACK__';

export function installBackNavigationBridge() {
    window[BACK_HANDLER_KEY] = handleBackNavigation;
}

function handleBackNavigation() {
    if (consumeBackNavigationHandlers()) {
        return true;
    }

    if (closeTopmostDialogPopup()) {
        return true;
    }

    if (closeOverlayByCloseButton()) {
        return true;
    }

    if (closeFloatingPanel()) {
        return true;
    }

    if (triggerBlankClickIfNeeded()) {
        return true;
    }

    if (closeChatIfOpen()) {
        return true;
    }

    return false;
}

function closeTopmostDialogPopup() {
    const dialogs = Array.from(document.querySelectorAll('dialog[open]:not([closing])'));
    if (dialogs.length === 0) {
        return false;
    }

    const dialog = dialogs[dialogs.length - 1];
    const closeButton = dialog.querySelector('.popup-button-close');
    if (closeButton instanceof HTMLElement) {
        closeButton.click();
        return true;
    }

    if (dialog instanceof HTMLDialogElement) {
        dialog.close();
        return true;
    }

    return false;
}

function closeOverlayByCloseButton() {
    const dialoguePopup = document.getElementById('dialogue_popup');
    if (isVisible(dialoguePopup)) {
        const cancelButton = document.getElementById('dialogue_popup_cancel');
        if (isVisible(cancelButton)) {
            cancelButton.click();
            return true;
        }

        const okButton = document.getElementById('dialogue_popup_ok');
        if (isVisible(okButton)) {
            okButton.click();
            return true;
        }

        return false;
    }

    const selectChatPopup = document.getElementById('select_chat_popup');
    if (isVisible(selectChatPopup)) {
        const closeButton = document.getElementById('select_chat_cross');
        if (closeButton instanceof HTMLElement) {
            closeButton.click();
            return true;
        }

        return false;
    }

    const characterPopup = document.getElementById('character_popup');
    if (isVisible(characterPopup)) {
        const closeButton = document.getElementById('character_cross');
        if (closeButton instanceof HTMLElement) {
            closeButton.click();
            return true;
        }

        return false;
    }

    const deleteMessageCancel = document.getElementById('dialogue_del_mes_cancel');
    if (isVisible(deleteMessageCancel)) {
        deleteMessageCancel.click();
        return true;
    }

    const logprobsViewer = document.getElementById('logprobsViewer');
    if (isVisible(logprobsViewer)) {
        const closeButton = document.getElementById('logprobsViewerClose');
        if (closeButton instanceof HTMLElement) {
            closeButton.click();
            return true;
        }

        return false;
    }

    return false;
}

function closeFloatingPanel() {
    const closeButtons = Array.from(document.querySelectorAll('#movingDivs .floating_panel_close'));
    for (let index = closeButtons.length - 1; index >= 0; index -= 1) {
        const closeButton = closeButtons[index];
        if (isVisible(closeButton)) {
            closeButton.click();
            return true;
        }
    }

    return false;
}

function triggerBlankClickIfNeeded() {
    const hasOpenDrawer = Boolean(document.querySelector('.openDrawer:not(.pinnedOpen)'));
    const hasOptionsMenu = isVisible(document.getElementById('options'));
    const hasExtraMessageButtons = Boolean(document.querySelector('.extraMesButtons.visible'));
    const hasActionModal = Array.from(document.querySelectorAll('.actionButtonsModal')).some(isVisible);

    if (!hasOpenDrawer && !hasOptionsMenu && !hasExtraMessageButtons && !hasActionModal) {
        return false;
    }

    dispatchBlankClick();
    return true;
}

function closeChatIfOpen() {
    const welcomePanel = document.querySelector('#chat > div.welcomePanel');
    if (isVisible(welcomePanel)) {
        return false;
    }

    const closeChatButtons = Array.from(document.querySelectorAll('#option_close_chat'));
    if (closeChatButtons.length === 0) {
        return false;
    }

    const closeChatButton = closeChatButtons[closeChatButtons.length - 1];
    if (!(closeChatButton instanceof HTMLElement)) {
        return false;
    }

    closeChatButton.click();
    return true;
}

function dispatchBlankClick() {
    const root = document.documentElement;
    root.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    root.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

function isVisible(element) {
    if (!(element instanceof HTMLElement)) {
        return false;
    }

    if (element.getClientRects().length === 0) {
        return false;
    }

    const style = getComputedStyle(element);
    return style.visibility !== 'hidden';
}
