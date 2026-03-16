import { isMobileRuntime } from './platform.js';
import {
    DEFAULT_CHAT_WINDOW_LINES_DESKTOP,
    DEFAULT_CHAT_WINDOW_LINES_MOBILE,
} from './windowed-defaults.js';

let currentWindow = null;

export {
    DEFAULT_CHAT_WINDOW_LINES_DESKTOP,
    DEFAULT_CHAT_WINDOW_LINES_MOBILE,
} from './windowed-defaults.js';
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

function requireWindowedCounter(value, label) {
    const normalized = Number(value);
    if (!Number.isFinite(normalized) || normalized < 0) {
        throw new Error(label);
    }
    return normalized;
}

export function readWindowedMessageSaveState(windowState, label = 'chat') {
    return {
        savedMessageCount: requireWindowedCounter(
            windowState?.savedMessageCount,
            `Windowed ${label} savedMessageCount is missing`,
        ),
        dirtyFromIndex: requireWindowedCounter(
            windowState?.dirtyFromIndex,
            `Windowed ${label} dirtyFromIndex is missing`,
        ),
    };
}

export function shiftWindowedMessageSaveState(windowState, deltaMessages, label = 'chat') {
    const { savedMessageCount, dirtyFromIndex } = readWindowedMessageSaveState(windowState, label);
    const delta = requireWindowedCounter(deltaMessages, `Invalid windowed ${label} prepend count`);

    return {
        ...windowState,
        savedMessageCount: savedMessageCount + delta,
        dirtyFromIndex: dirtyFromIndex + delta,
    };
}

export function buildWindowedPayloadPatch(messages, windowState, label = 'chat') {
    if (!Array.isArray(messages)) {
        throw new Error(`Windowed ${label} messages are missing`);
    }

    const { savedMessageCount, dirtyFromIndex } = readWindowedMessageSaveState(windowState, label);

    /** @type {string[]} */
    let lines;
    /** @type {{ kind: 'append', lines: string[] } | { kind: 'rewriteFromIndex', startIndex: number, lines: string[] }} */
    let patch;

    if (dirtyFromIndex < savedMessageCount) {
        lines = messages.slice(dirtyFromIndex).map((entry) => JSON.stringify(entry));
        patch = { kind: 'rewriteFromIndex', startIndex: dirtyFromIndex, lines };
    } else if (messages.length < savedMessageCount) {
        patch = { kind: 'rewriteFromIndex', startIndex: messages.length, lines: [] };
    } else if (messages.length > savedMessageCount) {
        lines = messages.slice(savedMessageCount).map((entry) => JSON.stringify(entry));
        patch = { kind: 'append', lines };
    } else {
        patch = messages.length === 0
            ? { kind: 'append', lines: [] }
            : { kind: 'rewriteFromIndex', startIndex: 0, lines: messages.map((entry) => JSON.stringify(entry)) };
    }

    return {
        patch,
        savedMessageCount: messages.length,
        dirtyFromIndex: messages.length,
    };
}
