import { translateAgentSystem as tr } from './i18n';

type SillyTavernPopupContext = {
    Popup?: {
        show?: {
            confirm?: (title: string | null, message: string) => unknown;
        };
    };
    POPUP_RESULT?: { AFFIRMATIVE: unknown };
};

type SillyTavernWindow = Window & {
    SillyTavern?: { getContext?: () => unknown };
};

function getSillyTavernContext(): unknown {
    return (window as SillyTavernWindow).SillyTavern?.getContext?.();
}

export function waitForHostReady(): Promise<void> {
    const ready = window.__TAURITAVERN__?.ready
        ?? window.__TAURITAVERN_MAIN_READY__;
    if (ready == null) {
        throw new Error(tr('hostReadyUnavailable'));
    }
    return ready;
}

export function requireHostApi<K extends keyof TauriTavernHostApi>(name: K): NonNullable<TauriTavernHostApi[K]> {
    const api = window.__TAURITAVERN__?.api?.[name];
    if (!api) {
        throw new Error(tr('hostApiUnavailable', { name }));
    }
    return api;
}

export function requireAgentApi(): TauriTavernAgentApi {
    const agent = requireHostApi('agent');
    if (!agent.profiles) {
        throw new Error(tr('hostAgentProfileApiUnavailable'));
    }
    return agent;
}

export function requireSkillApi(): TauriTavernSkillApi {
    return requireHostApi('skill');
}

export function requireLlmConnectionsApi(): TauriTavernLlmConnectionsApi {
    return requireHostApi('llmConnections');
}

export function requireSillyTavernContext(): unknown {
    const context = getSillyTavernContext();
    if (!context) {
        throw new Error(tr('sillyTavernContextUnavailable'));
    }
    return context;
}

export async function confirmAction(message: string): Promise<boolean> {
    const context = getSillyTavernContext() as SillyTavernPopupContext | null | undefined;
    const Popup = context?.Popup;
    const POPUP_RESULT = context?.POPUP_RESULT;
    if (!Popup?.show?.confirm || !POPUP_RESULT) {
        throw new Error(tr('hostPopupApiUnavailable'));
    }

    return await Popup.show.confirm(null, message) === POPUP_RESULT.AFFIRMATIVE;
}

export function clone<T>(value: T): T {
    return structuredClone(value);
}

export function prettyJson(value: unknown): string {
    return JSON.stringify(value, null, 2);
}

export function errorText(error: unknown): string {
    const value = typeof error === 'object' && error !== null && 'message' in error
        ? error.message
        : error;
    if (typeof value === 'string') {
        return value || tr('unknownError');
    }
    if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
        return String(value);
    }
    return tr('unknownError');
}

export function reportAgentSystemError(error: unknown): string {
    const message = errorText(error);
    console.error('[AgentSystem]', error);
    window.toastr?.error?.(message);
    return message;
}
