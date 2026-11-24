/**
 * Boundary contract of the request inspector. The view DTOs are the host's own
 * (`src/types.d.ts`); this module only names them and validates the JavaScript
 * boundary.
 */

export type Translate = (key: string) => string;
export type Unsubscribe = () => void | Promise<void>;

export type LogIndexEntry = TauriTavernLlmApiLogIndexEntry;
export type LogView = TauriTavernLlmApiLogView;
export type LogRaw = TauriTavernLlmApiLogRaw;
export type RequestPreview = TauriTavernRequestPreview;
export type ExchangeView = TauriTavernExchangeView;
export type RequestView = TauriTavernRequestView;
export type MessageView = TauriTavernMessageView;
export type PartView = TauriTavernPartView;
export type RequestDescription = TauriTavernRequestDescription;
export type PromptSource = TauriTavernPromptSource;
export type PromptPiece = TauriTavernPromptPiece;

/** The readable text of a logged exchange, as `llmApiLogs.getPreview` renders it. */
export type LogText = Pick<TauriTavernLlmApiLogPreview, 'requestReadable' | 'responseReadable'>;

/** `null` id: the log is selected while nothing has been recorded yet. */
export type Selection = { kind: 'preview' } | { kind: 'log'; id: number | null };

export type RequestInspectorClient = {
    index: (options?: { limit?: number }) => Promise<LogIndexEntry[]>;
    getLogView: (id: number) => Promise<LogView>;
    getLogText: (id: number) => Promise<LogText>;
    getRaw: (id: number) => Promise<LogRaw>;
    subscribeIndex: (handler: (entry: LogIndexEntry) => void) => Promise<Unsubscribe>;
    setKeep: (value: number) => Promise<void>;
    preview: () => Promise<RequestPreview>;
};

export type TextViewerOptions = {
    title: string;
    text: string;
    wrap: 'soft' | 'off';
};

export type RequestInspectorActions = {
    copyText: (text: string) => Promise<unknown>;
    openTextViewer: (options: TextViewerOptions) => Promise<unknown>;
    reportError: (error: unknown) => void;
    /** A host error as the reader should see it. */
    errorText: (error: unknown) => string;
};

export type RequestInspectorMountOptions = {
    keep: number;
    entries: LogIndexEntry[];
    /** `latest` selects the newest logged request. */
    select: 'preview' | 'latest';
    client: RequestInspectorClient;
    actions: RequestInspectorActions;
    tr: Translate;
};

export type RequestInspectorHandle = {
    unmount: () => void;
};

const CLIENT_METHODS = ['index', 'getLogView', 'getLogText', 'getRaw', 'subscribeIndex', 'setKeep', 'preview'];
const ACTIONS = ['copyText', 'openTextViewer', 'reportError', 'errorText'];

/** Validates the parts of the JS host boundary that TypeScript cannot see. */
export function validateRequestInspectorBoundary(
    options: unknown,
): asserts options is RequestInspectorMountOptions {
    if (!plainObject(options) || typeof options.tr !== 'function') {
        throw new Error('TauriTavern request inspector translator is required');
    }
    if (!Number.isSafeInteger(options.keep) || !Array.isArray(options.entries)) {
        throw new Error('TauriTavern request inspector log index is required');
    }
    if (options.select !== 'preview' && options.select !== 'latest') {
        throw new Error('TauriTavern request inspector selection is required');
    }
    requireMethods(options.client, CLIENT_METHODS, 'client method');
    requireMethods(options.actions, ACTIONS, 'action');
}

function requireMethods(target: unknown, names: string[], kind: string): void {
    for (const name of names) {
        if (!plainObject(target) || typeof target[name] !== 'function') {
            throw new Error(`TauriTavern request inspector ${kind} is unavailable: ${name}`);
        }
    }
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
