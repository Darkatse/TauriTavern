const PREFIX = 'mcp_manager.';
const DEFAULT_MESSAGES = Object.freeze({
    active: 'Active',
    activate: 'Activate',
    activateHttpNote: 'This endpoint uses unencrypted HTTP. Other devices on the network may observe or modify MCP traffic, including custom headers. Activation allows manual discovery and test calls, but does not grant tool permission.',
    activateNote: 'Activation allows manual discovery and test calls to this exact endpoint and sends its configured custom headers. It does not grant tool permission.',
    activateTitle: 'Activate this MCP server?',
    addHeader: 'Add header',
    addServer: 'Add server',
    addServerTitle: 'Add MCP server',
    adding: 'Adding…',
    advanced: 'Advanced',
    cancel: 'Cancel',
    close: 'Close',
    configuredToolsMissing: 'Not offered by this discovery',
    customDescription: 'Custom description',
    customDescriptionHint: 'Replaces the server description in what the model sees. Leave empty to use the server description.',
    customHeaders: 'Custom headers',
    customized: 'Custom',
    diagnostics: 'Discovery notes',
    discoverTools: 'Discover tools',
    discovering: 'Discovering tools…',
    discoveryIdentity: '{implementation} · MCP {protocol}',
    displayName: 'Name',
    displayNamePlaceholder: 'Local tools',
    edit: 'Edit',
    editDescription: 'Edit description',
    editServerTitle: 'Edit MCP server',
    emptyHint: 'Add a Streamable HTTP endpoint to see the tools it offers.',
    emptyTitle: 'No MCP servers yet',
    endpoint: 'Endpoint',
    endpointHint: 'Streamable HTTP endpoint.',
    endpointInvalid: 'Enter a valid http:// or https:// URL.',
    fieldRequired: 'Required',
    headerName: 'Header name',
    headerValue: 'Header value',
    headersPlaintext: 'Endpoint credentials and header values are stored in plaintext and included in data backups.',
    hostApiUnavailable: 'TauriTavern MCP Host API is unavailable',
    inputMode: 'Input mode',
    invalidInteger: 'Enter a whole number.',
    invalidJson: 'Enter valid JSON.',
    invalidNumber: 'Enter a valid number.',
    json: 'JSON',
    jsonConfig: 'MCP JSON',
    jsonHint: 'Paste one Streamable HTTP server, as a direct object or under mcpServers.',
    jsonInvalid: 'Invalid MCP JSON: {message}',
    loadingTools: 'Loading tools…',
    manual: 'Manual',
    mcp: 'MCP',
    nameRequired: 'Enter a name.',
    newServerNote: 'New servers start paused, and every discovered tool starts Off.',
    noActiveServers: 'No active servers. Activate a server in the list first.',
    noArguments: 'This tool takes no arguments.',
    noDisplayableContent: 'The server responded, but there is no displayable content.',
    notSent: 'Not sent',
    notSet: 'Not set',
    onePerLine: 'One value per line.',
    outcomeUnknown: 'Outcome unknown',
    outcomeUnknownHint: 'The call may have executed. TauriTavern will not retry it automatically.',
    paused: 'Paused',
    pausedHint: 'Paused — activate to discover and test this server\'s tools.',
    permissionAllow: 'Allow',
    permissionAsk: 'Ask',
    permissionFor: 'Permission for {name}',
    permissionOff: 'Off',
    popupUnavailable: 'SillyTavern Popup API is unavailable',
    protocolAuto: 'Auto (recommended)',
    protocolHint: 'Auto negotiates the newest mutually supported version.',
    protocolVersion: 'Protocol version',
    refreshTools: 'Refresh tools',
    remove: 'Remove',
    removeHeader: 'Remove header',
    removeNote: 'The registration and all saved tool settings will be removed.',
    removeTitle: 'Remove this MCP server?',
    resetCustomization: 'Reset all customizations',
    retry: 'Retry',
    runTest: 'Run test',
    schemaDetails: 'Schema & hints',
    selectServer: 'Server',
    selectServerPlaceholder: 'Select a server…',
    selectTool: 'Tool',
    selectToolPlaceholder: 'Select a tool…',
    serverDescription: 'Server description',
    serverError: 'Server error',
    serverResponded: 'Server responded',
    save: 'Save',
    saving: 'Saving…',
    serverCount: 'Servers · {count}',
    setOff: 'Set Off',
    storageIssues: 'Registration files needing attention',
    stopWaiting: 'Stop waiting',
    stopping: 'Stopping local wait…',
    structuredResult: 'Structured result',
    testCall: 'Test call',
    testCallPermission: 'Current permission: {permission}. This explicit test call does not change it.',
    testCallTitle: 'Test MCP tool',
    testCallWarning: 'This is a real call and may have side effects. TauriTavern never retries it automatically.',
    toolError: 'Tool returned an error',
    toolCount: '{count} tools',
    toggleTools: 'Show or hide tools',
    unknownError: 'Unknown error',
    unsupportedResponse: 'Unsupported server response',
    waitingForServer: 'Waiting for server…',
    waitingForServerHint: 'Stopping only ends the local wait; it cannot undo a call the server already received.',
});
export type McpMessageKey = keyof typeof DEFAULT_MESSAGES;
export type McpMessageParams = Readonly<Record<string, string | number>>;
export type McpTranslator = (key: McpMessageKey, params?: McpMessageParams) => string;

type PopupInstance = {
    result: unknown;
    show: () => Promise<unknown>;
};

type PopupOptions = {
    okButton?: string;
    cancelButton?: string;
    customButtons?: string[];
    allowVerticalScrolling?: boolean;
    wide?: boolean;
    leftAlign?: boolean;
    onOpen?: () => void;
    onClosing?: (popup: PopupInstance) => boolean | Promise<boolean>;
};

type PopupConstructor = new (
    content: Element,
    type: number,
    inputValue: string,
    options: PopupOptions,
) => PopupInstance;

type SillyTavernContext = {
    translate?: (fallback: string, key?: string) => string;
    Popup?: PopupConstructor;
    POPUP_TYPE?: { TEXT: number; CONFIRM: number; INPUT: number };
    POPUP_RESULT?: { AFFIRMATIVE: unknown };
};

type SillyTavernWindow = Window & {
    SillyTavern?: {
        getContext?: () => SillyTavernContext;
    };
};

function context(): SillyTavernContext | null {
    return (window as SillyTavernWindow).SillyTavern?.getContext?.() ?? null;
}

function requirePopup(): PopupConstructor {
    const Popup = context()?.Popup;
    if (!Popup) {
        throw new Error(tr('popupUnavailable'));
    }
    return Popup;
}

function formatMessage(message: string, params: McpMessageParams): string {
    return message.replace(/\{(\w+)\}/g, (match, name: string) => (
        Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match
    ));
}

export const tr: McpTranslator = (key, params = {}) => {
    const fallback = DEFAULT_MESSAGES[key];
    const message = context()?.translate?.(fallback, `${PREFIX}${key}`) ?? fallback;
    return formatMessage(message, params);
};

export function errorText(error: unknown, fallback: string): string {
    if (error instanceof Error) {
        return error.message;
    }
    return typeof error === 'string' && error ? error : fallback;
}

export async function waitForHostReady(): Promise<void> {
    const ready = window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__;
    if (ready !== undefined && ready !== null) {
        await ready;
    }
}

export function requireMcpApi(): TauriTavernMcpApi {
    const api = window.__TAURITAVERN__?.api?.mcp;
    if (!api) {
        throw new Error(tr('hostApiUnavailable'));
    }
    return api;
}

function createTypedPopup(content: Element, options: PopupOptions, type: number | undefined): PopupInstance {
    if (type === undefined) {
        throw new Error(tr('popupUnavailable'));
    }
    const Popup = requirePopup();
    const onClosing = options.onClosing;
    let closing = false;
    return new Popup(content, type, '', onClosing ? {
        ...options,
        onClosing: async popup => {
            if (closing) {
                return false;
            }
            closing = true;
            try {
                return await onClosing(popup);
            } finally {
                closing = false;
            }
        },
    } : options);
}

export function createTextPopup(content: Element, options: PopupOptions): PopupInstance {
    return createTypedPopup(content, options, context()?.POPUP_TYPE?.TEXT);
}

export function createConfirmPopup(content: Element, options: PopupOptions): PopupInstance {
    return createTypedPopup(content, options, context()?.POPUP_TYPE?.CONFIRM);
}

export function isPopupAffirmative(result: unknown): boolean {
    const affirmative = context()?.POPUP_RESULT?.AFFIRMATIVE;
    return affirmative !== undefined && result === affirmative;
}

function confirmationContent(title: string, endpoint: string, note: string): HTMLElement {
    const content = document.createElement('div');
    content.className = 'tt-mcp-confirm';

    const heading = document.createElement('h3');
    heading.textContent = title;
    const endpointCode = document.createElement('code');
    endpointCode.textContent = endpoint;
    const detail = document.createElement('p');
    detail.textContent = note;

    content.append(heading, endpointCode, detail);
    return content;
}

async function confirm(content: HTMLElement, okButton: string): Promise<boolean> {
    const popup = createConfirmPopup(content, {
        okButton,
        cancelButton: tr('cancel'),
        allowVerticalScrolling: true,
    });
    return isPopupAffirmative(await popup.show());
}

export async function confirmActivate(server: TauriTavernMcpServer): Promise<boolean> {
    const insecureHttp = new URL(server.endpoint).protocol === 'http:';
    return confirm(
        confirmationContent(
            tr('activateTitle'),
            server.endpoint,
            tr(insecureHttp ? 'activateHttpNote' : 'activateNote'),
        ),
        tr('activate'),
    );
}

export async function confirmRemove(server: TauriTavernMcpServer): Promise<boolean> {
    return confirm(
        confirmationContent(tr('removeTitle'), server.endpoint, tr('removeNote')),
        tr('remove'),
    );
}
