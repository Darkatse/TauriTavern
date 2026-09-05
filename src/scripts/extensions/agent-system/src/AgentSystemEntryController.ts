import { DEFAULT_AGENT_PROFILE_ID } from '../../../tauritavern/agent/agent-system-settings.js';
import type { AgentSystemTr } from './i18n';
import type { AgentSystemSettings } from './settings-store';

export type AgentSystemEntrySnapshot = {
    loading: boolean;
    settings: AgentSystemSettings;
    profiles: TauriTavernAgentProfileSummary[];
};

export type AgentSystemEntryControllerDeps = {
    loadSettings: () => Promise<AgentSystemSettings>;
    patchSettings: (current: AgentSystemSettings, patch: Partial<AgentSystemSettings>) => Promise<AgentSystemSettings>;
    subscribeSettings: (listener: (settings: AgentSystemSettings) => void) => () => void;
    listProfiles: () => Promise<TauriTavernAgentProfileSummary[]>;
    subscribeProfilesChanged: (listener: () => void) => () => void;
    notifyError: (error: unknown) => void;
    notifyWarning: (message: string) => void;
    tr: AgentSystemTr;
};

export type AgentSystemEntryController = {
    getSnapshot: () => AgentSystemEntrySnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    dispose: () => void;
    toggleAgentMode: () => Promise<void>;
    toggleChatInputToggleVisibility: () => Promise<void>;
    setActiveProfile: (profileId: string) => Promise<void>;
};

const DEFAULT_SETTINGS: AgentSystemSettings = {
    agentModeEnabled: false,
    chatInputToggleHidden: false,
    activeProfileId: DEFAULT_AGENT_PROFILE_ID,
    editingProfileId: DEFAULT_AGENT_PROFILE_ID,
    activeTab: 'profiles',
    runTimelineHeightPx: null,
};

/**
 * Mount-local owner of the settings-entry state. The composition root calls
 * init() exactly once as part of startup; React only subscribes to the
 * published snapshot, so StrictMode re-mounts cannot
 * duplicate Host subscriptions or writes.
 */
export function createAgentSystemEntryController(deps: AgentSystemEntryControllerDeps): AgentSystemEntryController {
    let snapshot: AgentSystemEntrySnapshot = {
        loading: false,
        settings: { ...DEFAULT_SETTINGS },
        profiles: [],
    };
    const listeners = new Set<() => void>();
    const unsubscribes: Array<() => void> = [];
    let disposed = false;
    let initPromise: Promise<void> | null = null;

    function unsubscribeAll(): void {
        unsubscribes.splice(0).forEach((unsubscribe) => unsubscribe());
    }

    function commit(patch: Partial<AgentSystemEntrySnapshot>): void {
        snapshot = { ...snapshot, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    function activeProfileOptions(): TauriTavernAgentProfileSummary[] {
        return snapshot.profiles.filter((profile) => profile.directRunnable !== false);
    }

    function reportAndRethrow(error: unknown): never {
        deps.notifyError(error);
        throw error;
    }

    // Subscription callbacks are fire-and-forget; failures stay visible via
    // toastr and surface as unhandled rejections for the dev-log capture.
    function runEventTask(task: () => Promise<void>): void {
        void (async () => {
            try {
                await task();
            } catch (error) {
                deps.notifyError(error);
                queueMicrotask(() => {
                    throw error;
                });
            }
        })();
    }

    async function refreshProfiles(): Promise<void> {
        const profiles = await deps.listProfiles();
        if (disposed) {
            return;
        }
        commit({ profiles });
    }

    async function setActiveProfile(profileId: string): Promise<void> {
        const id = profileId.trim();
        const profile = snapshot.profiles.find((item) => item.id === id);
        if (!profile) {
            throw new Error(deps.tr('agentProfileNotFound', { id }));
        }
        if (profile.directRunnable === false) {
            throw new Error(deps.tr('agentProfileNotDirectRunnable', { id }));
        }
        const settings = await deps.patchSettings(snapshot.settings, { activeProfileId: id });
        if (disposed) {
            return;
        }
        commit({ settings });
    }

    async function ensureActiveProfileSelectable(): Promise<void> {
        const activeId = snapshot.settings.activeProfileId || DEFAULT_AGENT_PROFILE_ID;
        if (activeProfileOptions().some((profile) => profile.id === activeId)) {
            return;
        }
        const previousProfileId = activeId;
        await setActiveProfile(DEFAULT_AGENT_PROFILE_ID);
        if (disposed) {
            return;
        }
        if (previousProfileId !== DEFAULT_AGENT_PROFILE_ID) {
            deps.notifyWarning(deps.tr('activeProfileResetToDefault'));
        }
    }

    async function init(): Promise<void> {
        initPromise ??= (async () => {
            commit({ loading: true });
            try {
                const settings = await deps.loadSettings();
                if (disposed) {
                    return;
                }
                commit({ settings });
                await refreshProfiles();
                if (disposed) {
                    return;
                }
                await ensureActiveProfileSelectable();
                if (disposed) {
                    return;
                }
                unsubscribes.push(deps.subscribeSettings((settings) => {
                    if (disposed) {
                        return;
                    }
                    commit({ settings });
                }));
                unsubscribes.push(deps.subscribeProfilesChanged(() => {
                    runEventTask(async () => {
                        await refreshProfiles();
                        if (disposed) {
                            return;
                        }
                        await ensureActiveProfileSelectable();
                    });
                }));
            } catch (error) {
                unsubscribeAll();
                throw error;
            } finally {
                if (!disposed) {
                    commit({ loading: false });
                }
            }
        })();
        return initPromise;
    }

    function dispose(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        unsubscribeAll();
        listeners.clear();
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
            };
        },
        init,
        dispose,
        async toggleAgentMode() {
            try {
                const settings = await deps.patchSettings(snapshot.settings, {
                    agentModeEnabled: !snapshot.settings.agentModeEnabled,
                });
                if (disposed) {
                    return;
                }
                commit({ settings });
            } catch (error) {
                reportAndRethrow(error);
            }
        },
        async toggleChatInputToggleVisibility() {
            try {
                const settings = await deps.patchSettings(snapshot.settings, {
                    chatInputToggleHidden: !snapshot.settings.chatInputToggleHidden,
                });
                if (disposed) {
                    return;
                }
                commit({ settings });
            } catch (error) {
                reportAndRethrow(error);
            }
        },
        async setActiveProfile(profileId) {
            try {
                await setActiveProfile(profileId);
            } catch (error) {
                reportAndRethrow(error);
            }
        },
    };
}
