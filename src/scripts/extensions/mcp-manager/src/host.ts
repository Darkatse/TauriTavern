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
    customHeaders: 'Custom headers',
    diagnostics: 'Discovery notes',
    discoverTools: 'Discover tools',
    discovering: 'Discovering tools…',
    discoveryIdentity: '{implementation} · MCP {protocol}',
    displayName: 'Name',
    displayNamePlaceholder: 'Local tools',
    edit: 'Edit',
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
    refreshTools: 'Refresh tools',
    remove: 'Remove',
    removeHeader: 'Remove header',
    removeNote: 'The registration and all saved tool permissions will be removed.',
    removeTitle: 'Remove this MCP server?',
    protocolAuto: 'Auto (recommended)',
    protocolHint: 'Auto negotiates the newest mutually supported version.',
    protocolVersion: 'Protocol version',
    retry: 'Retry',
    runTest: 'Run test',
    schemaDetails: 'Schema & hints',
    selectServer: 'Server',
    selectServerPlaceholder: 'Select a server…',
    selectTool: 'Tool',
    selectToolPlaceholder: 'Select a tool…',
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

function requirePopupRuntime(): { Popup: PopupConstructor; affirmative: unknown } {
    const host = context();
    const Popup = host?.Popup;
    const affirmative = host?.POPUP_RESULT?.AFFIRMATIVE;
    if (!Popup || affirmative === undefined) {
        throw new Error(tr('popupUnavailable'));
    }
    return { Popup, affirmative };
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
    const { Popup, affirmative } = requirePopupRuntime();
    const confirmType = context()?.POPUP_TYPE?.CONFIRM;
    if (confirmType === undefined) {
        throw new Error(tr('popupUnavailable'));
    }

    const popup = new Popup(content, confirmType, '', {
        okButton,
        cancelButton: tr('cancel'),
        allowVerticalScrolling: true,
    });
    return await popup.show() === affirmative;
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

function serverInput(
    parent: HTMLElement,
    id: string,
    labelText: string,
    placeholder: string,
): HTMLInputElement {
    const label = document.createElement('label');
    label.htmlFor = id;
    label.textContent = labelText;

    const input = document.createElement('input');
    input.id = id;
    input.className = 'text_pole result-control';
    input.dataset.result = '1';
    input.dataset.resultEvent = 'submit';
    input.type = 'text';
    input.placeholder = placeholder;
    input.autocomplete = 'off';
    parent.append(label, input);
    return input;
}

function isHttpUrl(value: string): boolean {
    try {
        const url = new URL(value);
        return url.protocol === 'http:' || url.protocol === 'https:';
    } catch {
        return false;
    }
}

type CreateMcpServerInput = Parameters<TauriTavernMcpApi['servers']['create']>[0];
type ServerDraft = CreateMcpServerInput & { protocolVersion: TauriTavernMcpProtocolVersion };

const PROTOCOL_VERSIONS: TauriTavernMcpProtocolVersion[] = [
    'auto',
    '2026-07-28',
    '2025-11-25',
    '2025-06-18',
    '2025-03-26',
];

function parseProtocolVersion(value: unknown): TauriTavernMcpProtocolVersion {
    const version = value ?? 'auto';
    if (typeof version !== 'string' || !PROTOCOL_VERSIONS.includes(version as TauriTavernMcpProtocolVersion)) {
        throw new Error('unsupported protocol version');
    }
    return version as TauriTavernMcpProtocolVersion;
}

function jsonObject(value: unknown, label: string): Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value as Record<string, unknown>;
}

export function parseMcpServerJson(raw: string): ServerDraft {
    const root = jsonObject(JSON.parse(raw), 'root');
    const servers = Object.prototype.hasOwnProperty.call(root, 'mcpServers')
        ? jsonObject(root.mcpServers, 'mcpServers')
        : root;
    const entries = Object.entries(servers);
    if (entries.length !== 1) {
        throw new Error('configuration must contain exactly one server');
    }

    const entry = entries.pop();
    if (!entry) {
        throw new Error('configuration must contain exactly one server');
    }
    const [rawName, rawConfig] = entry;
    const displayName = rawName.trim();
    const config = jsonObject(rawConfig, displayName || 'server');
    const endpoint = typeof config.url === 'string' ? config.url.trim() : '';
    if (!displayName) {
        throw new Error('server name is required');
    }
    if (!isHttpUrl(endpoint)) {
        throw new Error('server url must be an HTTP(S) URL');
    }
    if (config.type !== undefined) {
        if (typeof config.type !== 'string') {
            throw new Error('transport type must be a string');
        }
        const transport = config.type.toLowerCase();
        if (!['http', 'streamable-http', 'streamable_http'].includes(transport)) {
            throw new Error(`unsupported transport type: ${config.type}`);
        }
    }

    const rawHeaders = config.headers === undefined ? {} : jsonObject(config.headers, 'headers');
    const headers = Object.fromEntries(Object.entries(rawHeaders).map(([name, value]) => {
        if (typeof value !== 'string') {
            throw new Error(`header ${name} must be a string`);
        }
        return [name, value];
    }));
    return {
        displayName,
        endpoint,
        headers,
        protocolVersion: parseProtocolVersion(config.protocolVersion),
    };
}

async function openServerDialog(
    save: (draft: ServerDraft) => Promise<TauriTavernMcpServer>,
    server?: TauriTavernMcpServer,
): Promise<TauriTavernMcpServer | null> {
    const { Popup, affirmative } = requirePopupRuntime();
    const confirmType = context()?.POPUP_TYPE?.CONFIRM;
    if (confirmType === undefined) {
        throw new Error(tr('popupUnavailable'));
    }

    const form = document.createElement('div');
    form.className = 'tt-mcp-add-form';

    const heading = document.createElement('h3');
    heading.textContent = tr(server ? 'editServerTitle' : 'addServerTitle');
    form.append(heading);

    const tabs = document.createElement('div');
    tabs.className = 'tt-mcp-seg tt-mcp-add-tabs';
    tabs.setAttribute('role', 'radiogroup');
    tabs.setAttribute('aria-label', tr('inputMode'));
    function modeTab(value: 'manual' | 'json', labelText: string, iconClass: string, checked: boolean) {
        const label = document.createElement('label');
        label.classList.toggle('is-selected', checked);
        const input = document.createElement('input');
        input.type = 'radio';
        input.name = 'tt-mcp-add-mode';
        input.value = value;
        input.checked = checked;
        const text = document.createElement('span');
        const icon = document.createElement('i');
        icon.className = `fa-solid ${iconClass}`;
        icon.setAttribute('aria-hidden', 'true');
        text.append(icon, document.createTextNode(labelText));
        label.append(input, text);
        tabs.append(label);
        return { label, input };
    }
    const manualTab = modeTab('manual', tr('manual'), 'fa-pen-to-square', true);
    const jsonTab = modeTab('json', tr('json'), 'fa-code', false);

    const manualPane = document.createElement('div');
    manualPane.className = 'tt-mcp-add-pane';
    const displayName = serverInput(
        manualPane,
        'tt-mcp-add-name',
        tr('displayName'),
        tr('displayNamePlaceholder'),
    );
    displayName.value = server?.displayName ?? '';

    const endpoint = serverInput(
        manualPane,
        'tt-mcp-add-endpoint',
        tr('endpoint'),
        'http://127.0.0.1:3000/mcp',
    );
    endpoint.inputMode = 'url';
    endpoint.value = server?.endpoint ?? '';

    const notes = server ? [tr('endpointHint')] : [tr('endpointHint'), tr('newServerNote')];
    for (const text of notes) {
        const hint = document.createElement('small');
        hint.className = 'tt-mcp-add-hint';
        hint.textContent = text;
        manualPane.append(hint);
    }

    const advanced = document.createElement('details');
    // Not tt-mcp-details: that class shrinks console diagnostics to 0.8em, while the
    // dialog's Advanced section must keep the form's base size so its labels, inputs
    // and hints compose to the same sizes as the rest of the dialog.
    advanced.className = 'tt-mcp-advanced';
    const advancedSummary = document.createElement('summary');
    advancedSummary.textContent = tr('advanced');
    advanced.append(advancedSummary);

    const protocolLabel = document.createElement('label');
    protocolLabel.htmlFor = 'tt-mcp-protocol-version';
    protocolLabel.textContent = tr('protocolVersion');
    const protocolVersion = document.createElement('select');
    protocolVersion.id = 'tt-mcp-protocol-version';
    protocolVersion.className = 'text_pole';
    for (const version of PROTOCOL_VERSIONS) {
        const option = document.createElement('option');
        option.value = version;
        option.textContent = version === 'auto' ? tr('protocolAuto') : version;
        protocolVersion.append(option);
    }
    protocolVersion.value = server?.protocolVersion ?? 'auto';
    const protocolHint = document.createElement('small');
    protocolHint.className = 'tt-mcp-add-hint';
    protocolHint.textContent = tr('protocolHint');
    advanced.append(protocolLabel, protocolVersion, protocolHint);

    const headersHead = document.createElement('div');
    headersHead.className = 'tt-mcp-header-head';
    const headersLabel = document.createElement('span');
    headersLabel.textContent = tr('customHeaders');
    const addHeaderButton = document.createElement('button');
    addHeaderButton.type = 'button';
    addHeaderButton.className = 'menu_button menu_button_icon';
    addHeaderButton.innerHTML = '<i class="fa-solid fa-plus" aria-hidden="true"></i>';
    const addHeaderText = document.createElement('span');
    addHeaderText.textContent = tr('addHeader');
    addHeaderButton.append(addHeaderText);
    headersHead.append(headersLabel, addHeaderButton);

    const headerList = document.createElement('div');
    headerList.className = 'tt-mcp-header-list';
    const headerRows = new Set<{ name: HTMLInputElement; value: HTMLInputElement }>();
    function addHeader(headerName = '', headerValue = '', focus = true): void {
        const row = document.createElement('div');
        row.className = 'tt-mcp-header-row';
        const name = document.createElement('input');
        name.type = 'text';
        name.className = 'text_pole';
        name.placeholder = 'x-api-key';
        name.autocomplete = 'off';
        name.setAttribute('aria-label', tr('headerName'));
        name.value = headerName;
        const value = document.createElement('input');
        value.type = 'password';
        value.className = 'text_pole';
        value.placeholder = tr('headerValue');
        value.autocomplete = 'off';
        value.setAttribute('aria-label', tr('headerValue'));
        value.value = headerValue;
        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'tt-mcp-icon-btn is-danger';
        remove.title = tr('removeHeader');
        remove.setAttribute('aria-label', tr('removeHeader'));
        remove.innerHTML = '<i class="fa-solid fa-trash-can" aria-hidden="true"></i>';
        const fields = { name, value };
        remove.addEventListener('click', () => {
            headerRows.delete(fields);
            row.remove();
        });
        row.append(name, value, remove);
        headerRows.add(fields);
        headerList.append(row);
        if (focus) {
            name.focus();
        }
    }
    addHeaderButton.addEventListener('click', () => addHeader());
    for (const [name, value] of Object.entries(server?.headers ?? {})) {
        addHeader(name, value, false);
    }
    const plaintextHint = document.createElement('small');
    plaintextHint.className = 'tt-mcp-add-hint';
    plaintextHint.textContent = tr('headersPlaintext');
    advanced.append(headersHead, headerList, plaintextHint);
    // Reflect the current state: editing a server with advanced settings reveals them.
    advanced.open = server !== undefined
        && (Object.keys(server.headers).length > 0 || server.protocolVersion !== 'auto');
    manualPane.append(advanced);

    const jsonPane = document.createElement('div');
    jsonPane.className = 'tt-mcp-add-pane';
    jsonPane.hidden = true;
    const jsonLabel = document.createElement('label');
    jsonLabel.htmlFor = 'tt-mcp-add-json';
    jsonLabel.textContent = tr('jsonConfig');
    const jsonInput = document.createElement('textarea');
    jsonInput.id = 'tt-mcp-add-json';
    jsonInput.className = 'text_pole tt-mcp-json-input';
    jsonInput.spellcheck = false;
    jsonInput.placeholder = '{\n  "exa": {\n    "url": "https://mcp.exa.ai/mcp",\n    "headers": { "x-api-key": "YOUR_EXA_API_KEY" }\n  }\n}';
    const jsonHint = document.createElement('small');
    jsonHint.className = 'tt-mcp-add-hint';
    jsonHint.textContent = tr('jsonHint');
    const jsonPlaintextHint = plaintextHint.cloneNode(true);
    jsonPane.append(jsonLabel, jsonInput, jsonHint, jsonPlaintextHint);

    let mode: 'manual' | 'json' = 'manual';
    function setMode(next: 'manual' | 'json'): void {
        mode = next;
        manualPane.hidden = next !== 'manual';
        jsonPane.hidden = next !== 'json';
        manualTab.label.classList.toggle('is-selected', next === 'manual');
        jsonTab.label.classList.toggle('is-selected', next === 'json');
        (next === 'manual' ? displayName : jsonInput).focus();
    }
    manualTab.input.addEventListener('change', () => {
        if (manualTab.input.checked) {
            setMode('manual');
        }
    });
    jsonTab.input.addEventListener('change', () => {
        if (jsonTab.input.checked) {
            setMode('json');
        }
    });
    if (server) {
        form.append(manualPane);
    } else {
        form.append(tabs, manualPane, jsonPane);
    }

    const error = document.createElement('p');
    error.className = 'tt-mcp-error';
    error.setAttribute('role', 'alert');

    const busyNote = document.createElement('p');
    busyNote.className = 'tt-mcp-note';
    const spinner = document.createElement('i');
    spinner.className = 'fa-solid fa-circle-notch fa-spin';
    spinner.setAttribute('aria-hidden', 'true');
    busyNote.append(spinner, tr(server ? 'saving' : 'adding'));

    function showError(message: string): void {
        error.remove();
        error.textContent = message;
        if (message) {
            form.insertBefore(error, busyNote.parentElement === form ? busyNote : null);
        }
    }

    function setBusy(value: boolean): void {
        for (const control of form.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | HTMLButtonElement>('input, textarea, select, button')) {
            control.disabled = value;
        }
        if (value) {
            form.append(busyNote);
        } else {
            busyNote.remove();
        }
    }

    function manualDraft(): ServerDraft {
        const name = displayName.value.trim();
        const target = endpoint.value.trim();
        if (!name) {
            throw new Error(tr('nameRequired'));
        }
        if (!isHttpUrl(target)) {
            throw new Error(tr('endpointInvalid'));
        }
        return {
            displayName: name,
            endpoint: target,
            headers: Object.fromEntries([...headerRows].map(({ name, value }) => [name.value, value.value])),
            protocolVersion: parseProtocolVersion(protocolVersion.value),
        };
    }

    let saved: TauriTavernMcpServer | null = null;
    let submitting = false;
    const popup = new Popup(form, confirmType, '', {
        okButton: tr(server ? 'save' : 'addServer'),
        cancelButton: tr('cancel'),
        allowVerticalScrolling: true,
        onOpen: () => (mode === 'manual' ? displayName : jsonInput).focus(),
        onClosing: async (popup) => {
            if (popup.result !== affirmative) {
                return true;
            }

            if (submitting) {
                return false;
            }
            let draft: ServerDraft;
            try {
                draft = mode === 'manual' ? manualDraft() : parseMcpServerJson(jsonInput.value);
            } catch (cause) {
                // A validation error may point into the collapsed Advanced section — reveal it.
                advanced.open = true;
                const message = errorText(cause, tr('unknownError'));
                showError(mode === 'json' ? tr('jsonInvalid', { message }) : message);
                return false;
            }

            showError('');
            submitting = true;
            setBusy(true);
            try {
                saved = await save(draft);
                return true;
            } catch (cause) {
                showError(errorText(cause, tr('unknownError')));
                return false;
            } finally {
                submitting = false;
                setBusy(false);
            }
        },
    });
    await popup.show();
    return saved;
}

export function openAddServerDialog(
    create: TauriTavernMcpApi['servers']['create'],
): Promise<TauriTavernMcpServer | null> {
    return openServerDialog(draft => create(draft));
}

export function openEditServerDialog(
    server: TauriTavernMcpServer,
    update: TauriTavernMcpApi['servers']['update'],
): Promise<TauriTavernMcpServer | null> {
    return openServerDialog(draft => update({
        registrationId: server.id,
        displayName: draft.displayName,
        endpoint: draft.endpoint,
        headers: draft.headers ?? {},
        protocolVersion: draft.protocolVersion,
    }), server);
}

export function createTextPopup(content: Element, options: PopupOptions): PopupInstance {
    const { Popup } = requirePopupRuntime();
    const textType = context()?.POPUP_TYPE?.TEXT;
    if (textType === undefined) {
        throw new Error(tr('popupUnavailable'));
    }
    return new Popup(content, textType, '', options);
}
