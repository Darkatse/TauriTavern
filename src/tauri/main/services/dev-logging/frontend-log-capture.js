// @ts-check

import { invoke } from '../../../../tauri-bridge.js';
import { createSameOriginIframeLogCapture } from './same-origin-iframe-log-capture.js';

const CONSOLE_CAPTURE_STORAGE_KEY = 'tt:devConsoleCapture';

const BUFFER_LIMIT = 2000;
const FLUSH_INTERVAL_MS = 250;

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
    return detectLogTargetFromStack(stackToLines(new Error().stack));
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
/** @type {FrontendLogEntry[]} */
let pendingFlush = [];
let flushTimer = /** @type {ReturnType<typeof setTimeout> | null} */ (null);

/** @type {Partial<Record<keyof Console, (...args: any[]) => void>> | null} */
let originalConsole = null;
let consoleCaptureEnabled = readConsoleCaptureBootstrapFlag();

function readConsoleCaptureBootstrapFlag() {
    try {
        return globalThis.localStorage?.getItem(CONSOLE_CAPTURE_STORAGE_KEY) === '1';
    } catch {
        return false;
    }
}

/** @param {boolean} enabled */
function writeConsoleCaptureBootstrapFlag(enabled) {
    try {
        globalThis.localStorage?.setItem(CONSOLE_CAPTURE_STORAGE_KEY, enabled ? '1' : '0');
    } catch {
        // Ignore storage write failures.
    }
}

/** @param {FrontendLogEntry} entry */
function notify(entry) {
    for (const handler of subscribers) {
        try {
            handler(entry);
        } catch {
            // Ignore subscriber failures.
        }
    }
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
        message,
        ...(target ? { target } : {}),
    };

    entries.push(entry);
    if (entries.length > BUFFER_LIMIT) {
        entries.splice(0, entries.length - BUFFER_LIMIT);
    }

    notify(entry);

    pendingFlush.push(entry);
    if (backendForwardingEnabled) {
        scheduleFlush();
    }
}

function scheduleFlush() {
    if (flushTimer) {
        return;
    }

    flushTimer = setTimeout(() => {
        flushTimer = null;
        void flushPending();
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
    if (!backendForwardingEnabled || pendingFlush.length === 0) {
        pendingFlush = [];
        return;
    }

    const batch = pendingFlush;
    pendingFlush = [];

    try {
        await invoke('devlog_append_frontend_logs', {
            entries: batch.map((entry) => ({
                level: entry.level,
                message: entry.message,
                ...(entry.target ? { target: entry.target } : {}),
            })),
        });
    } catch (error) {
        reportFlushError(error);
    }
}

/** @param {any[]} args */
function formatConsoleArgs(args) {
    const parts = [];
    for (const arg of args) {
        if (typeof arg === 'string') {
            parts.push(arg);
            continue;
        }

        const stack = arg && typeof arg === 'object' ? arg.stack : null;
        const message = arg && typeof arg === 'object' ? arg.message : null;
        if (typeof stack === 'string' && stack) {
            parts.push(stack);
            continue;
        }
        if (typeof message === 'string' && message) {
            parts.push(message);
            continue;
        }

        try {
            parts.push(JSON.stringify(arg));
        } catch {
            parts.push(String(arg));
        }
    }

    return parts.join(' ');
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
    if (backendForwardingEnabled && pendingFlush.length > 0) {
        scheduleFlush();
    }
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
