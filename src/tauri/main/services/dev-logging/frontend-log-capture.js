// @ts-check

import { invoke } from '../../../../tauri-bridge.js';
import { createSameOriginIframeLogCapture } from './same-origin-iframe-log-capture.js';
import { trimFrontendLogEntriesInPlace } from './frontend-log-retention.js';

const CONSOLE_CAPTURE_STORAGE_KEY = 'tt:devConsoleCapture';

const FLUSH_INTERVAL_MS = 250;
const MAX_MESSAGE_CHARS = 3072;

/** @typedef {'debug' | 'info' | 'warn' | 'error'} FrontendLogLevel */
/** @typedef {{ id: number, timestampMs: number, level: FrontendLogLevel, message: string, target?: string }} FrontendLogEntry */

const DEFAULT_LOG_TARGET = 'main';

/**
 * @param {string | undefined} stack
 */
function stackToLines(stack) {
    if (typeof stack !== 'string' || !stack) {
        return [];
    }
    return stack.split('\n').map((line) => line.trim());
}

/**
 * Best-effort semantic source detection aligned with SillyTavern extension conventions.
 * @param {string[]} stackLines
 */
function detectLogTargetFromStack(stackLines) {
    const thirdPartyLine = stackLines.find((line) => line.includes('/scripts/extensions/third-party/'));
    if (thirdPartyLine) {
        const match = thirdPartyLine.match(/\/scripts\/extensions\/third-party\/([^/]+)\//);
        return match ? `3p:${match[1]}` : '3p';
    }

    const extensionLine = stackLines.find((line) => line.includes('/scripts/extensions/'));
    if (extensionLine) {
        const match = extensionLine.match(/\/scripts\/extensions\/([^/]+)\//);
        return match ? `ext:${match[1]}` : 'ext';
    }

    const scriptsLine = stackLines.find((line) => line.includes('/scripts/'));
    if (scriptsLine) {
        const match = scriptsLine.match(/\/(scripts\/[^?#):]+?\.(?:js|mjs|cjs))/);
        if (match) {
            return match[1];
        }
    }

    const tauriLine = stackLines.find((line) =>
        line.includes('/tauri/') && !line.includes('/services/dev-logging/'),
    );
    if (tauriLine) {
        const match = tauriLine.match(/\/(tauri\/[^?#):]+?\.(?:js|mjs|cjs))/);
        if (match) {
            return match[1];
        }
    }

    return DEFAULT_LOG_TARGET;
}

function detectCurrentLogTarget() {
    return DEFAULT_LOG_TARGET;
}

/**
 * @param {unknown} error
 */
function detectLogTargetFromError(error) {
    const stack = error && typeof error === 'object' ? /** @type {any} */ (error).stack : null;
    if (typeof stack === 'string' && stack) {
        return detectLogTargetFromStack(stackToLines(stack));
    }
    return DEFAULT_LOG_TARGET;
}

/** @type {FrontendLogEntry[]} */
const entries = [];
/** @type {Set<(entry: FrontendLogEntry) => void>} */
const subscribers = new Set();

let nextId = 1;
let backendForwardingEnabled = false;
let flushTimer = /** @type {ReturnType<typeof setTimeout> | null} */ (null);
let lastForwardedId = 0;

/** @type {Partial<Record<keyof Console, (...args: any[]) => void>> | null} */
let originalConsole = null;
let consoleCaptureEnabled = readConsoleCaptureBootstrapFlag();

function readConsoleCaptureBootstrapFlag() {
    try {
        return globalThis.localStorage?.getItem(CONSOLE_CAPTURE_STORAGE_KEY) === '1';
    } catch (error) {
        console.warn('TauriTavern: Failed to read dev console capture flag:', error);
        return false;
    }
}

/** @param {boolean} enabled */
function writeConsoleCaptureBootstrapFlag(enabled) {
    try {
        globalThis.localStorage?.setItem(CONSOLE_CAPTURE_STORAGE_KEY, enabled ? '1' : '0');
    } catch (error) {
        const warnFn = originalConsole?.warn ?? console.warn;
        warnFn('TauriTavern: Failed to persist dev console capture flag:', error);
    }
}

/** @param {FrontendLogEntry} entry */
function notify(entry) {
    for (const handler of subscribers) {
        try {
            handler(entry);
        } catch (error) {
            // Keep capture running, but make the failure visible.
            const errorFn = originalConsole?.error ?? console.error;
            errorFn('TauriTavern: frontend log subscriber failed:', error);
        }
    }
}

/**
 * @param {string} value
 * @param {number} maxChars
 */
function truncateString(value, maxChars) {
    const text = String(value ?? '');
    if (text.length <= maxChars) {
        return text;
    }
    if (maxChars <= 1) {
        return '…';
    }
    return `${text.slice(0, maxChars - 1)}…`;
}

/**
 * @param {unknown} value
 * @param {number} budget
 */
function formatConsoleArgPreview(value, budget) {
    try {
        if (budget <= 0) {
            return '';
        }

        if (value === null) {
            return 'null';
        }
        if (value === undefined) {
            return 'undefined';
        }

        const valueType = typeof value;
        if (valueType === 'string') {
            return truncateString(/** @type {string} */ (value), budget);
        }
        if (valueType === 'number' || valueType === 'boolean' || valueType === 'bigint') {
            return truncateString(String(value), budget);
        }
        if (valueType === 'symbol') {
            return truncateString(value.toString(), budget);
        }
        if (valueType === 'function') {
            const fn = /** @type {Function} */ (value);
            const name = String(fn.name || '').trim();
            return truncateString(name ? `[Function ${name}]` : '[Function]', budget);
        }

        if (valueType !== 'object') {
            return truncateString(String(value), budget);
        }

        const obj = /** @type {any} */ (value);

        const stack = obj?.stack;
        if (typeof stack === 'string' && stack) {
            return truncateString(stack, budget);
        }

        const message = obj?.message;
        if (typeof message === 'string' && message) {
            return truncateString(message, budget);
        }

        if (Array.isArray(obj)) {
            return truncateString(`Array(${obj.length})`, budget);
        }

        if (obj instanceof Map) {
            return truncateString(`Map(${obj.size})`, budget);
        }

        if (obj instanceof Set) {
            return truncateString(`Set(${obj.size})`, budget);
        }

        if (ArrayBuffer.isView(obj)) {
            const view = /** @type {ArrayBufferView} */ (obj);
            const name = view.constructor?.name || 'ArrayBufferView';
            const maybeLength = /** @type {any} */ (view).length;
            const length = typeof maybeLength === 'number' ? maybeLength : 0;
            return truncateString(`${name}(${length})`, budget);
        }

        let displayed = 0;
        const keys = [];
        for (const key in obj) {
            if (!Object.prototype.hasOwnProperty.call(obj, key)) {
                continue;
            }
            keys.push(key);
            displayed += 1;
            if (displayed >= 6) {
                break;
            }
        }

        const ctorName = String(obj?.constructor?.name || 'Object').trim() || 'Object';
        if (keys.length === 0) {
            return truncateString(ctorName, budget);
        }
        return truncateString(`${ctorName}{${keys.join(',')}}`, budget);
    } catch (error) {
        const errorFn = originalConsole?.error ?? console.error;
        errorFn('TauriTavern: Failed to format console log args:', error);
        return '[LogFormatError]';
    }
}

/** @param {any[]} args */
function formatConsoleArgs(args) {
    let out = '';

    for (const arg of args) {
        if (out.length >= MAX_MESSAGE_CHARS) {
            break;
        }

        const separator = out ? ' ' : '';
        const budget = MAX_MESSAGE_CHARS - out.length - separator.length;
        if (budget <= 0) {
            break;
        }

        const part = formatConsoleArgPreview(arg, budget);
        if (!part) {
            continue;
        }

        out = `${out}${separator}${part}`;
    }

    return truncateString(out, MAX_MESSAGE_CHARS);
}

/**
 * @param {FrontendLogLevel} level
 * @param {string} message
 * @param {string | undefined} [target]
 */
function push(level, message, target) {
    const entry = {
        id: nextId++,
        timestampMs: Date.now(),
        level,
        message: truncateString(String(message ?? ''), MAX_MESSAGE_CHARS),
        ...(target ? { target } : {}),
    };

    entries.push(entry);
    trimFrontendLogEntriesInPlace(entries);

    notify(entry);
    scheduleFlush();
}

function scheduleFlush() {
    if (!backendForwardingEnabled) {
        return;
    }

    if (flushTimer) {
        return;
    }

    flushTimer = setTimeout(() => {
        void (async () => {
            try {
                await flushPending();
            } finally {
                flushTimer = null;
                const lastEntry = entries.length > 0 ? entries[entries.length - 1] : null;
                if (backendForwardingEnabled && lastEntry && lastEntry.id > lastForwardedId) {
                    scheduleFlush();
                }
            }
        })();
    }, FLUSH_INTERVAL_MS);
}

/** @param {unknown} error */
function reportFlushError(error) {
    const errorFn = originalConsole?.error;
    if (typeof errorFn === 'function') {
        errorFn('TauriTavern: Failed to forward frontend logs:', error);
        return;
    }

    console.error('TauriTavern: Failed to forward frontend logs:', error);
}

async function flushPending() {
    if (!backendForwardingEnabled) {
        return;
    }

    const batch = entries.filter((entry) => entry.id > lastForwardedId);
    if (batch.length === 0) {
        return;
    }

    try {
        await invoke('devlog_append_frontend_logs', {
            entries: batch.map((entry) => ({
                level: entry.level,
                message: entry.message,
                ...(entry.target ? { target: entry.target } : {}),
            })),
        });
        const last = /** @type {FrontendLogEntry} */ (batch[batch.length - 1]);
        lastForwardedId = last.id;
    } catch (error) {
        reportFlushError(error);
    }
}

const iframeLogCapture = createSameOriginIframeLogCapture({
    push,
    formatConsoleArgs,
    isConsoleCaptureEnabled: () => consoleCaptureEnabled,
});

function captureWindowErrors() {
    globalThis.addEventListener('error', (event) => {
        const message = String(event?.message || 'Unknown error');
        const errorStack = event?.error && typeof event.error === 'object' ? event.error.stack : null;
        const errorMessage = event?.error && typeof event.error === 'object' ? event.error.message : null;
        const details = typeof errorStack === 'string'
            ? `\n${errorStack}`
            : typeof errorMessage === 'string'
                ? `\n${errorMessage}`
                : '';
        push('error', `${message}${details}`, detectLogTargetFromError(event?.error));
    });

    globalThis.addEventListener('unhandledrejection', (event) => {
        const reason = event?.reason;
        const stack = reason && typeof reason === 'object' ? reason.stack : null;
        const message = reason && typeof reason === 'object' ? reason.message : null;
        if (typeof stack === 'string' && stack) {
            push('error', `Unhandled rejection: ${stack}`, detectLogTargetFromError(reason));
            return;
        }
        if (typeof message === 'string' && message) {
            push('error', `Unhandled rejection: ${message}`, detectLogTargetFromError(reason));
            return;
        }
        push('error', `Unhandled rejection: ${String(reason)}`, detectLogTargetFromError(reason));
    });
}

function patchConsole() {
    if (originalConsole) {
        return;
    }

    originalConsole = {
        debug: console.debug?.bind(console),
        log: console.log?.bind(console),
        info: console.info?.bind(console),
        warn: console.warn?.bind(console),
        error: console.error?.bind(console),
    };

    if (originalConsole.debug) {
        console.debug = (...args) => {
            originalConsole?.debug?.(...args);
            push('debug', formatConsoleArgs(args), detectCurrentLogTarget());
        };
    }

    if (originalConsole.log) {
        console.log = (...args) => {
            originalConsole?.log?.(...args);
            push('info', formatConsoleArgs(args), detectCurrentLogTarget());
        };
    }

    if (originalConsole.info) {
        console.info = (...args) => {
            originalConsole?.info?.(...args);
            push('info', formatConsoleArgs(args), detectCurrentLogTarget());
        };
    }

    if (originalConsole.warn) {
        console.warn = (...args) => {
            originalConsole?.warn?.(...args);
            push('warn', formatConsoleArgs(args), detectCurrentLogTarget());
        };
    }

    if (originalConsole.error) {
        console.error = (...args) => {
            originalConsole?.error?.(...args);
            push('error', formatConsoleArgs(args), detectCurrentLogTarget());
        };
    }
}

function restoreConsole() {
    if (!originalConsole) {
        return;
    }

    if (originalConsole.debug) console.debug = originalConsole.debug;
    if (originalConsole.log) console.log = originalConsole.log;
    if (originalConsole.info) console.info = originalConsole.info;
    if (originalConsole.warn) console.warn = originalConsole.warn;
    if (originalConsole.error) console.error = originalConsole.error;

    originalConsole = null;
}

export function installFrontendLogCapture() {
    captureWindowErrors();
    iframeLogCapture.install();

    if (consoleCaptureEnabled) {
        patchConsole();
    }
}

/** @param {boolean} enabled */
export function setFrontendLogBackendForwardingEnabled(enabled) {
    backendForwardingEnabled = Boolean(enabled);
    scheduleFlush();
}

export function isFrontendConsoleCaptureEnabled() {
    return consoleCaptureEnabled;
}

/** @param {boolean} enabled */
export function setFrontendConsoleCaptureEnabled(enabled) {
    consoleCaptureEnabled = Boolean(enabled);
    writeConsoleCaptureBootstrapFlag(consoleCaptureEnabled);

    if (consoleCaptureEnabled) {
        patchConsole();
        iframeLogCapture.scan();
        scheduleFlush();
        return;
    }

    restoreConsole();
    iframeLogCapture.restore();
}

export function getFrontendLogEntries() {
    return entries.slice();
}

/**
 * @param {(entry: FrontendLogEntry) => void} handler
 */
export function subscribeFrontendLogs(handler) {
    subscribers.add(handler);
    return () => subscribers.delete(handler);
}
