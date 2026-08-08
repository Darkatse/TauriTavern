const PREFIX = 'mcp_manager.';

const DEFAULT_MESSAGES = Object.freeze({
    active: 'Active',
    activate: 'Activate',
    activateHttpNote: 'This endpoint uses unencrypted HTTP. Other devices on the network may observe or modify MCP traffic. Activation does not grant tool permission.',
    activateNote: 'Activation allows manual discovery requests to this exact endpoint. It does not grant tool permission.',
    activateTitle: 'Activate this MCP server?',
    addServer: 'Add server',
    addServerTitle: 'Add MCP server',
    adding: 'Adding…',
    cancel: 'Cancel',
    configuredToolsMissing: 'Not offered by this discovery',
    diagnostics: 'Discovery notes',
    discoverTools: 'Discover tools',
    discovering: 'Discovering tools…',
    discoveryIdentity: '{implementation} · MCP {protocol}',
    displayName: 'Name',
    displayNamePlaceholder: 'Local tools',
    emptyHint: 'Add a Streamable HTTP endpoint to see the tools it offers.',
    emptyTitle: 'No MCP servers yet',
    endpoint: 'Endpoint',
    endpointHint: 'Streamable HTTP. This endpoint is the registration identity and cannot be changed later.',
    endpointInvalid: 'Enter a valid http:// or https:// URL.',
    hostApiUnavailable: 'TauriTavern MCP Host API is unavailable',
    mcp: 'MCP',
    nameRequired: 'Enter a name.',
    newServerNote: 'New servers start paused, and every discovered tool starts Off.',
    paused: 'Paused',
    pausedHint: 'Paused — activate to discover this server\'s tools.',
    permissionAllow: 'Allow',
    permissionAsk: 'Ask',
    permissionFor: 'Permission for {name}',
    permissionOff: 'Off',
    popupUnavailable: 'SillyTavern Popup API is unavailable',
    refreshTools: 'Refresh tools',
    remove: 'Remove',
    removeNote: 'The registration and all saved tool permissions will be removed.',
    removeTitle: 'Remove this MCP server?',
    rename: 'Rename',
    renameTitle: 'Rename server',
    retry: 'Retry',
    schemaDetails: 'Schema & hints',
    serverCount: 'Servers · {count}',
    setOff: 'Set Off',
    storageIssues: 'Registration files needing attention',
    toolCount: '{count} tools',
    toggleTools: 'Show or hide tools',
    unknownError: 'Unknown error',
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
    onOpen?: () => void;
    onClosing?: (popup: PopupInstance) => Promise<boolean>;
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
    POPUP_TYPE?: { CONFIRM: number; INPUT: number };
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

/**
 * Vanilla SillyTavern rename flow: an INPUT popup resolves the typed string on
 * OK, `false` on the negative button and `null` on cancel/Escape.
 * Returns null when the user cancelled or the name did not change.
 */
export async function promptServerName(server: TauriTavernMcpServer): Promise<string | null> {
    const { Popup } = requirePopupRuntime();
    const inputType = context()?.POPUP_TYPE?.INPUT;
    if (inputType === undefined) {
        throw new Error(tr('popupUnavailable'));
    }

    const content = document.createElement('h3');
    content.textContent = tr('renameTitle');

    const value = await new Popup(content, inputType, server.displayName, {
        okButton: tr('rename'),
        cancelButton: tr('cancel'),
    }).show();
    if (typeof value !== 'string') {
        return null;
    }
    const displayName = value.trim();
    return displayName.length > 0 && displayName !== server.displayName ? displayName : null;
}

function addServerInput(
    form: HTMLElement,
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
    form.append(label, input);
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

export async function openAddServerDialog(
    create: TauriTavernMcpApi['servers']['create'],
): Promise<TauriTavernMcpServer | null> {
    const { Popup, affirmative } = requirePopupRuntime();
    const confirmType = context()?.POPUP_TYPE?.CONFIRM;
    if (confirmType === undefined) {
        throw new Error(tr('popupUnavailable'));
    }

    const form = document.createElement('div');
    form.className = 'tt-mcp-add-form';

    const heading = document.createElement('h3');
    heading.textContent = tr('addServerTitle');
    form.append(heading);

    const displayName = addServerInput(
        form,
        'tt-mcp-add-name',
        tr('displayName'),
        tr('displayNamePlaceholder'),
    );
    displayName.maxLength = 128;

    const endpoint = addServerInput(
        form,
        'tt-mcp-add-endpoint',
        tr('endpoint'),
        'http://127.0.0.1:3000/mcp',
    );
    endpoint.inputMode = 'url';

    for (const text of [tr('endpointHint'), tr('newServerNote')]) {
        const hint = document.createElement('small');
        hint.className = 'tt-mcp-add-hint';
        hint.textContent = text;
        form.append(hint);
    }

    const error = document.createElement('p');
    error.className = 'tt-mcp-error';
    error.setAttribute('role', 'alert');

    const busyNote = document.createElement('p');
    busyNote.className = 'tt-mcp-note';
    const spinner = document.createElement('i');
    spinner.className = 'fa-solid fa-circle-notch fa-spin';
    spinner.setAttribute('aria-hidden', 'true');
    busyNote.append(spinner, tr('adding'));

    function showError(message: string): void {
        error.remove();
        error.textContent = message;
        if (message) {
            form.insertBefore(error, busyNote.parentElement === form ? busyNote : null);
        }
    }

    function setBusy(value: boolean): void {
        displayName.disabled = value;
        endpoint.disabled = value;
        if (value) {
            form.append(busyNote);
        } else {
            busyNote.remove();
        }
    }

    let created: TauriTavernMcpServer | null = null;
    let submitting = false;
    const popup = new Popup(form, confirmType, '', {
        okButton: tr('addServer'),
        cancelButton: tr('cancel'),
        allowVerticalScrolling: true,
        onOpen: () => displayName.focus(),
        onClosing: async (popup) => {
            if (popup.result !== affirmative) {
                return true;
            }

            if (submitting) {
                return false;
            }
            const name = displayName.value.trim();
            const target = endpoint.value.trim();
            if (!name) {
                showError(tr('nameRequired'));
                return false;
            }
            if (!isHttpUrl(target)) {
                showError(tr('endpointInvalid'));
                return false;
            }

            showError('');
            submitting = true;
            setBusy(true);
            try {
                created = await create({ displayName: name, endpoint: target });
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
    return created;
}
