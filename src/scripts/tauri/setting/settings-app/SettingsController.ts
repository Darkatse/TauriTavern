import {
    createSettingsDraft,
    type ChatBackupStorageStats,
    type SettingsDraft,
    type SettingsValues,
} from './SettingsContract';

/**
 * Mount-local owner of the unsaved Settings draft and the asynchronously
 * loaded chat backup storage stats. Both the React view (via
 * subscribe/getSnapshot) and the public mount handle (getDraft) read from
 * this store, so `getDraft()` never depends on a committed React render.
 * Every transition produces a new immutable snapshot and notifies listeners
 * synchronously.
 */

export type SettingsSnapshot = {
    draft: SettingsDraft;
    chatBackupStorageStats: ChatBackupStorageStats | null;
};

export type SettingsController = {
    getSnapshot: () => SettingsSnapshot;
    subscribe: (listener: () => void) => () => void;
    getDraft: () => SettingsDraft;
    updateDraft: <K extends keyof SettingsDraft>(field: K, value: SettingsDraft[K]) => void;
    patchChatBackups: (patch: Partial<SettingsDraft['chatBackups']>) => void;
    patchRequestProxy: (patch: Partial<SettingsDraft['requestProxy']>) => void;
    patchDynamicTheme: (patch: Partial<SettingsDraft['dynamicTheme']>) => void;
    setChatBackupStorageStats: (stats: ChatBackupStorageStats | null) => void;
};

export function createSettingsController({
    values,
    chatBackupStorageStats,
}: {
    values: SettingsValues;
    chatBackupStorageStats: ChatBackupStorageStats | null;
}): SettingsController {
    let state: SettingsSnapshot = {
        draft: createSettingsDraft(values),
        chatBackupStorageStats,
    };
    const listeners = new Set<() => void>();

    function getSnapshot(): SettingsSnapshot {
        return state;
    }

    function subscribe(listener: () => void): () => void {
        listeners.add(listener);
        return () => {
            listeners.delete(listener);
        };
    }

    function setState(patch: Partial<SettingsSnapshot>): void {
        state = { ...state, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    /** Returns a fresh nested copy so the popup shell can never mutate the store. */
    function getDraft(): SettingsDraft {
        const { draft } = state;
        return {
            ...draft,
            chatBackups: { ...draft.chatBackups },
            requestProxy: { ...draft.requestProxy },
            dynamicTheme: { ...draft.dynamicTheme },
        };
    }

    function updateDraft<K extends keyof SettingsDraft>(field: K, value: SettingsDraft[K]): void {
        setState({ draft: { ...state.draft, [field]: value } });
    }

    function patchChatBackups(patch: Partial<SettingsDraft['chatBackups']>): void {
        setState({ draft: { ...state.draft, chatBackups: { ...state.draft.chatBackups, ...patch } } });
    }

    function patchRequestProxy(patch: Partial<SettingsDraft['requestProxy']>): void {
        setState({ draft: { ...state.draft, requestProxy: { ...state.draft.requestProxy, ...patch } } });
    }

    function patchDynamicTheme(patch: Partial<SettingsDraft['dynamicTheme']>): void {
        setState({ draft: { ...state.draft, dynamicTheme: { ...state.draft.dynamicTheme, ...patch } } });
    }

    function setChatBackupStorageStats(stats: ChatBackupStorageStats | null): void {
        setState({ chatBackupStorageStats: stats });
    }

    return {
        getSnapshot,
        subscribe,
        getDraft,
        updateDraft,
        patchChatBackups,
        patchRequestProxy,
        patchDynamicTheme,
        setChatBackupStorageStats,
    };
}
