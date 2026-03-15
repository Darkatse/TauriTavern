const CHAT_INPUT_SELECTOR = '#send_textarea';
const RETAIN_RESTORE_SELECTOR = [
    '#options_button',
    '#send_but',
    '#mes_impersonate',
    '#mes_continue',
    '#send_textarea',
    '#option_regenerate',
    '#option_continue',
    '#option_toggle_fullscreen',
].join(', ');

const RESTORE_TRIGGER_SELECTOR = [
    '#send_but',
    '#option_regenerate',
    '#option_continue',
    '#mes_continue',
    '#mes_impersonate',
].join(', ');

const MOBILE_USER_AGENT_PATTERN = /android|iphone|ipad|ipod/i;

let focusKeeperInstalled = false;

export const ChatInputFocusIntent = Object.freeze({
    NAVIGATION: 'navigation',
    RESTORATION: 'restoration',
    EDITING: 'editing',
});

export function getChatInput() {
    const textarea = document.querySelector(CHAT_INPUT_SELECTOR);
    if (!(textarea instanceof HTMLTextAreaElement)) {
        throw new Error('Expected #send_textarea to exist');
    }

    return textarea;
}

export function isChatInputFocused() {
    return document.activeElement === getChatInput();
}

export function shouldFocusChatInput(intent) {
    if (!isMobileChatInputEnvironment()) {
        return true;
    }

    return intent !== ChatInputFocusIntent.NAVIGATION
        && intent !== ChatInputFocusIntent.RESTORATION;
}

export function focusChatInput(intent, { cursor = 'preserve' } = {}) {
    if (!shouldFocusChatInput(intent)) {
        return false;
    }

    const textarea = getChatInput();
    textarea.focus();

    if (cursor === 'end') {
        const selectionEnd = textarea.value.length;
        textarea.setSelectionRange(selectionEnd, selectionEnd);
    }

    return document.activeElement === textarea;
}

export function installChatInputFocusKeeper() {
    if (focusKeeperInstalled) {
        return;
    }

    focusKeeperInstalled = true;

    let shouldRestoreFocus = false;
    const textarea = getChatInput();

    const rememberChatInputFocus = () => {
        shouldRestoreFocus = true;
    };

    textarea.addEventListener('focusin', rememberChatInputFocus);
    textarea.addEventListener('focus', rememberChatInputFocus);
    textarea.addEventListener('click', rememberChatInputFocus);

    document.addEventListener('click', (event) => {
        const target = event.target;
        if (!(target instanceof Element)) {
            shouldRestoreFocus = isChatInputFocused();
            return;
        }

        if (shouldRestoreFocus && target.closest(RESTORE_TRIGGER_SELECTOR)) {
            focusChatInput(ChatInputFocusIntent.RESTORATION);
        }

        if (target.closest(CHAT_INPUT_SELECTOR) || isChatInputFocused()) {
            shouldRestoreFocus = true;
            return;
        }

        if (!target.closest(RETAIN_RESTORE_SELECTOR)) {
            shouldRestoreFocus = false;
        }
    });
}

function isMobileChatInputEnvironment() {
    if (typeof navigator === 'undefined') {
        return false;
    }

    const userAgent = typeof navigator.userAgent === 'string' ? navigator.userAgent : '';
    if (MOBILE_USER_AGENT_PATTERN.test(userAgent)) {
        return true;
    }

    return navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1;
}
