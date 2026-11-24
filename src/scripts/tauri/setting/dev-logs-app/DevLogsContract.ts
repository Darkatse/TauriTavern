/**
 * Boundary contract for the dev-logs-app feature.
 *
 * This module is the single home for the narrow types shared by the Dev Logs
 * mount and the runtime validation of the JavaScript host boundary. It contains
 * no state, no React and no host access.
 */

export type DevLogsTranslate = (key: string) => string;

/** A subscription disposer returned by the host; sync or async. */
export type DevLogsUnsubscribe = () => void | Promise<void>;

// ── Live log view DTO ───────────────────────────────────────────────────────

/**
 * The shape both live panels render. Frontend entries carry lowercase levels
 * and optional targets, backend entries uppercase levels and mandatory
 * targets; the view normalizes levels through log-utils.
 */
export type LiveLogEntry = {
    id?: number;
    timestampMs: number;
    level: string;
    message: string;
    target?: string;
};

// ── Host ports ──────────────────────────────────────────────────────────────

export type LiveLogClient = {
    subscribe: (handler: (entry: LiveLogEntry) => void) => Promise<DevLogsUnsubscribe>;
    /** Required by the boundary whenever `showConsoleCapture` is set. */
    setConsoleCaptureEnabled?: (enabled: boolean) => Promise<void>;
};

export type DevLogsActions = {
    copyText: (text: string) => Promise<unknown>;
    reportError: (error: unknown) => void;
};

// ── Mount options ───────────────────────────────────────────────────────────

export type DevLogsMountOptions = {
    title: string;
    initialEntries?: LiveLogEntry[];
    consoleCaptureEnabled?: boolean;
    showConsoleCapture?: boolean;
    trimEntriesInPlace?: ((entries: LiveLogEntry[]) => void) | null;
    client: LiveLogClient;
    actions: DevLogsActions;
    tr: DevLogsTranslate;
};

export type DevLogsHandle = {
    unmount: () => void;
};

// ── Boundary validation ─────────────────────────────────────────────────────

/** Validates the parts of the JS host boundary that TypeScript cannot see. */
export function validateDevLogsBoundary(
    options: unknown,
): asserts options is DevLogsMountOptions {
    if (!plainObject(options) || typeof options.tr !== 'function') {
        throw new Error('TauriTavern dev logs translator is required');
    }
    if (!plainObject(options.client)) {
        throw new Error('TauriTavern dev logs client is required');
    }
    if (!plainObject(options.actions)) {
        throw new Error('TauriTavern dev logs actions are required');
    }
    const requiredMethods = options.showConsoleCapture === true
        ? ['subscribe', 'setConsoleCaptureEnabled']
        : ['subscribe'];
    for (const name of requiredMethods) {
        if (typeof options.client[name] !== 'function') {
            throw new Error(`TauriTavern dev logs client method is unavailable: ${name}`);
        }
    }

    for (const name of ['copyText', 'reportError']) {
        if (typeof options.actions[name] !== 'function') {
            throw new Error(`TauriTavern dev logs action is unavailable: ${name}`);
        }
    }
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
