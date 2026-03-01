import { isMobileRuntime } from './platform.js';

let currentWindow = null;

export const DEFAULT_CHAT_WINDOW_LINES_DESKTOP = 100;
export const DEFAULT_CHAT_WINDOW_LINES_MOBILE = 50;
export const DEFAULT_CHAT_WINDOW_LINES = isMobileRuntime()
    ? DEFAULT_CHAT_WINDOW_LINES_MOBILE
    : DEFAULT_CHAT_WINDOW_LINES_DESKTOP;

export function getWindowedChatState() {
    return currentWindow;
}

export function setWindowedChatState(state) {
    currentWindow = state;
}

export function clearWindowedChatState() {
    currentWindow = null;
}
