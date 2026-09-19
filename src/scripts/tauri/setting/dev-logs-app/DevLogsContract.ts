/**
 * Boundary contract for the dev-logs-app feature.
 *
 * This module is the single home for the narrow types shared by the Dev Logs
 * mount, the runtime validation of the JavaScript host boundary, and the load
 * state unions both panels render from. It contains no state, no React and no
 * host access.
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

// ── LLM API log DTO (mirrors api.dev.llmApiLogs camelCase DTO) ─────────────

export type LlmApiLogLevel = 'INFO' | 'WARN' | 'ERROR';

export type LlmApiLogIndexEntry = {
    id: number;
    timestampMs: number;
    level: LlmApiLogLevel;
    ok: boolean;
    source: string;
    model: string | null;
    endpoint: string;
    durationMs: number;
    stream: boolean;
};

export type LlmApiLogPreview = LlmApiLogIndexEntry & {
    errorMessage: string | null;
    requestReadable: string;
    responseReadable: string;
    responseRawKind: 'json' | 'sse' | null;
};

export type LlmApiLogRaw = {
    id: number;
    requestRaw: string;
    responseRaw: string;
    responseRawKind: 'json' | 'sse' | null;
};

/**
 * The composition root hands over either a fetched preview or the explicit
 * load failure it observed while opening the popup.
 */
export type LlmApiLogPreviewResult = LlmApiLogPreview | { id: number; error: string };

/** Single-slot load projection; rendered only when `id` matches the selection. */
export type LlmApiLoadState<T> =
    | { id: number; status: 'loading' }
    | { id: number; status: 'ready'; value: T }
    | { id: number; status: 'error'; error: string };

export type PreviewLoadState = LlmApiLoadState<LlmApiLogPreview>;
export type RawLoadState = LlmApiLoadState<LlmApiLogRaw>;

/** Maps the boundary preview result onto the panel's initial load state. */
export function initialPreviewState(preview: LlmApiLogPreviewResult | null | undefined): PreviewLoadState | null {
    if (!preview) {
        return null;
    }
    if ('error' in preview) {
        return { id: preview.id, status: 'error', error: preview.error };
    }
    return { id: preview.id, status: 'ready', value: preview };
}

// ── Host ports ──────────────────────────────────────────────────────────────

export type LiveLogClient = {
    subscribe: (handler: (entry: LiveLogEntry) => void) => Promise<DevLogsUnsubscribe>;
    /** Required by the boundary whenever `showConsoleCapture` is set. */
    setConsoleCaptureEnabled?: (enabled: boolean) => Promise<void>;
};

export type LlmApiLogsClient = {
    index: (options?: { limit?: number }) => Promise<LlmApiLogIndexEntry[]>;
    getPreview: (id: number) => Promise<LlmApiLogPreview>;
    getRaw: (id: number) => Promise<LlmApiLogRaw>;
    subscribeIndex: (handler: (entry: LlmApiLogIndexEntry) => void) => Promise<DevLogsUnsubscribe>;
    setKeep: (value: number) => Promise<void>;
};

export type DevLogsTextViewerOptions = {
    title: string;
    text: string;
    wrap: 'soft' | 'off';
};

export type DevLogsActions = {
    copyText: (text: string) => Promise<unknown>;
    openTextViewer: (options: DevLogsTextViewerOptions) => Promise<unknown>;
    reportError: (error: unknown) => void;
};

// ── Mount options ───────────────────────────────────────────────────────────

export type LiveLogPanelOptions = {
    kind: 'live';
    title: string;
    initialEntries?: LiveLogEntry[];
    consoleCaptureEnabled?: boolean;
    showConsoleCapture?: boolean;
    trimEntriesInPlace?: ((entries: LiveLogEntry[]) => void) | null;
    client: LiveLogClient;
    actions: DevLogsActions;
    tr: DevLogsTranslate;
};

export type LlmApiLogsPanelOptions = {
    kind: 'llm-api';
    initialKeep?: number;
    initialIndexEntries?: LlmApiLogIndexEntry[];
    initialPreview?: LlmApiLogPreviewResult | null;
    client: LlmApiLogsClient;
    actions: DevLogsActions;
    tr: DevLogsTranslate;
};

export type DevLogsMountOptions = LiveLogPanelOptions | LlmApiLogsPanelOptions;

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
    if (options.kind !== 'live' && options.kind !== 'llm-api') {
        const { kind } = options;
        throw new Error(`Unsupported TauriTavern dev logs panel: ${typeof kind === 'string' ? kind : typeof kind}`);
    }

    const requiredMethods = options.kind === 'live'
        ? options.showConsoleCapture === true
            ? ['subscribe', 'setConsoleCaptureEnabled']
            : ['subscribe']
        : ['index', 'getPreview', 'getRaw', 'subscribeIndex', 'setKeep'];
    for (const name of requiredMethods) {
        if (typeof options.client[name] !== 'function') {
            throw new Error(`TauriTavern dev logs client method is unavailable: ${name}`);
        }
    }

    for (const name of ['copyText', 'openTextViewer', 'reportError']) {
        if (typeof options.actions[name] !== 'function') {
            throw new Error(`TauriTavern dev logs action is unavailable: ${name}`);
        }
    }
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
