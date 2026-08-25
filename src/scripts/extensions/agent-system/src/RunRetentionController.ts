import { errorText } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import type { Tr } from './AgentSystemPanelContract';

const RUN_PRUNE_DETAIL_LIMIT = 8;
export const MAX_AGENT_RETENTION_KEEP_RUNS = 10000;

/**
 * Transient draft: the number inputs hold raw text (string) while typing;
 * retentionSettingsFromDraft validates/converts at save/plan/apply time.
 */
export type RunRetentionDraft = {
    autoPruneEnabled: boolean;
    keepRecentTerminalRuns: number | string;
    keepFullRecentRuns: number | string;
};

export type RunRetentionSnapshot = {
    loading: boolean;
    saving: boolean;
    planning: boolean;
    applying: boolean;
    error: string;
    retention: TauriTavernAgentRunRetentionSettings | null;
    draft: RunRetentionDraft;
    plan: TauriTavernAgentRunPrunePlan | null;
};

export type RunRetentionControllerDeps = {
    getRetentionApi: () => TauriTavernAgentRetentionApi;
    confirmAction: (message: string) => Promise<boolean>;
    notifySuccess: (message: string) => void;
    notifyWarning: (message: string) => void;
    tr: Tr;
};

export type RunRetentionController = {
    getSnapshot: () => RunRetentionSnapshot;
    subscribe: (listener: () => void) => () => void;
    refresh: () => Promise<void>;
    save: () => Promise<void>;
    analyze: () => Promise<void>;
    // Resolves to the apply result on success (for the typed onPruned
    // callback), null on cancel/failure.
    applyPrune: () => Promise<TauriTavernAgentRunPruneApplyResult | null>;
    setAutoPruneEnabled: (enabled: boolean) => void;
    setKeepRecentTerminalRuns: (value: string) => void;
    setKeepFullRecentRuns: (value: string) => void;
    dispose: () => void;
};

export function retentionBusy(snapshot: RunRetentionSnapshot): boolean {
    return snapshot.loading || snapshot.saving || snapshot.planning || snapshot.applying;
}

export function retentionDraftIsDirty(snapshot: RunRetentionSnapshot): boolean {
    const retention = snapshot.retention;
    if (!retention) {
        return false;
    }
    try {
        const draft = retentionSettingsFromDraft(snapshot.draft);
        return draft.autoPruneEnabled !== retention.autoPruneEnabled
            || draft.keepRecentTerminalRuns !== retention.keepRecentTerminalRuns
            || draft.keepFullRecentRuns !== retention.keepFullRecentRuns;
    } catch {
        return true;
    }
}

export function retentionPlanHasWork(plan: TauriTavernAgentRunPrunePlan | null): boolean {
    return Number(plan?.totalCandidateFileCount || 0) > 0
        || Number(plan?.slimCandidateCount || 0) > 0
        || Number(plan?.deleteCandidateCount || 0) > 0;
}

export function retentionCanApplyPrune(snapshot: RunRetentionSnapshot): boolean {
    return Boolean(snapshot.plan && retentionPlanHasWork(snapshot.plan) && !retentionBusy(snapshot));
}

export function formatRetentionCount(value: unknown): string {
    return String(Number(value || 0));
}

export function formatRetentionFiles(value: unknown, translate: Tr): string {
    return translate('fileCount', { count: Number(value || 0) });
}

export function formatRetentionBytes(value: unknown): string {
    const bytes = Number(value || 0);
    if (!Number.isFinite(bytes) || bytes <= 0) {
        return '0 B';
    }
    const units = ['B', 'KB', 'MB', 'GB'];
    let size = bytes;
    let unitIndex = 0;
    while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex += 1;
    }
    const precision = unitIndex === 0 || size >= 10 ? 0 : 1;
    return `${size.toFixed(precision)} ${units[unitIndex] ?? 'B'}`;
}

/** Mount-local owner of the run retention editor + prune plan/apply flow. */
export function createRunRetentionController(deps: RunRetentionControllerDeps): RunRetentionController {
    let snapshot: RunRetentionSnapshot = {
        loading: false,
        saving: false,
        planning: false,
        applying: false,
        error: '',
        retention: null,
        draft: {
            autoPruneEnabled: false,
            keepRecentTerminalRuns: 100,
            keepFullRecentRuns: 20,
        },
        plan: null,
    };
    const listeners = new Set<() => void>();
    let disposed = false;

    function commit(patch: Partial<RunRetentionSnapshot>): void {
        if (disposed) {
            return;
        }
        snapshot = { ...snapshot, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    function applyRetention(retention: TauriTavernAgentRunRetentionSettings): void {
        commit({ retention, draft: { ...retention }, plan: null });
    }

    async function refresh(): Promise<void> {
        commit({ loading: true, error: '' });
        try {
            applyRetention(await deps.getRetentionApi().readSettings());
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            commit({ loading: false });
        }
    }

    async function save(): Promise<void> {
        commit({ saving: true, error: '' });
        try {
            const updated = await deps.getRetentionApi().updateSettings(retentionSettingsFromDraft(snapshot.draft));
            if (disposed) {
                return;
            }
            applyRetention(updated);
            deps.notifySuccess(deps.tr('runRetentionSaved'));
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            commit({ saving: false });
        }
    }

    async function analyze(): Promise<void> {
        commit({ planning: true, error: '' });
        try {
            const plan = await deps.getRetentionApi().planPrune({
                retention: retentionSettingsFromDraft(snapshot.draft),
                detailLimit: RUN_PRUNE_DETAIL_LIMIT,
            });
            if (disposed) {
                return;
            }
            commit({ plan });
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error), plan: null });
        } finally {
            commit({ planning: false });
        }
    }

    async function applyPrune(): Promise<TauriTavernAgentRunPruneApplyResult | null> {
        if (!retentionCanApplyPrune(snapshot)) {
            return null;
        }
        const plan = snapshot.plan;
        if (!plan) {
            return null;
        }

        commit({ error: '' });
        let confirmed: boolean;
        try {
            confirmed = await deps.confirmAction(deps.tr('runRetentionApplyConfirm', {
                bytes: formatRetentionBytes(plan.totalCandidateByteCount),
                files: formatRetentionFiles(plan.totalCandidateFileCount, deps.tr),
            }));
        } catch (error) {
            if (!disposed) {
                commit({ error: errorText(error) });
            }
            return null;
        }
        if (!confirmed || disposed) {
            return null;
        }

        commit({ applying: true });
        try {
            const result = await deps.getRetentionApi().applyPrune({
                retention: retentionSettingsFromDraft(snapshot.draft),
                detailLimit: RUN_PRUNE_DETAIL_LIMIT,
            });
            if (disposed) {
                return null;
            }
            commit({ plan: result.afterPlan });

            const toastParams = {
                bytes: formatRetentionBytes(result.removedByteCount),
                files: formatRetentionFiles(result.removedFileCount, deps.tr),
                count: Number(result.failedRunCount || 0),
            };
            if (Number(result.failedRunCount || 0) > 0) {
                deps.notifyWarning(deps.tr('runRetentionAppliedWithFailures', toastParams));
            } else {
                deps.notifySuccess(deps.tr('runRetentionApplied', toastParams));
            }
            return result;
        } catch (error) {
            if (!disposed) {
                commit({ error: errorText(error) });
            }
            return null;
        } finally {
            commit({ applying: false });
        }
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
            };
        },
        refresh,
        save,
        analyze,
        applyPrune,
        setAutoPruneEnabled(enabled: boolean): void {
            commit({ draft: { ...snapshot.draft, autoPruneEnabled: enabled }, plan: null });
        },
        setKeepRecentTerminalRuns(value: string): void {
            commit({ draft: { ...snapshot.draft, keepRecentTerminalRuns: value }, plan: null });
        },
        setKeepFullRecentRuns(value: string): void {
            commit({ draft: { ...snapshot.draft, keepFullRecentRuns: value }, plan: null });
        },
        dispose(): void {
            if (disposed) {
                return;
            }
            disposed = true;
            listeners.clear();
        },
    };
}

function retentionSettingsFromDraft(value: RunRetentionDraft): TauriTavernAgentRunRetentionSettings {
    const keepRecentTerminalRuns = normalizeRetentionCount(
        value.keepRecentTerminalRuns,
        'keepRecentTerminalRuns',
    );
    const keepFullRecentRuns = normalizeRetentionCount(
        value.keepFullRecentRuns,
        'keepFullRecentRuns',
    );
    if (keepFullRecentRuns > keepRecentTerminalRuns) {
        throw new Error(tr('runRetentionFullExceedsHistory'));
    }
    return {
        autoPruneEnabled: value.autoPruneEnabled,
        keepRecentTerminalRuns,
        keepFullRecentRuns,
    };
}

function normalizeRetentionCount(value: unknown, label: string): number {
    if (value == null || value === '') {
        throw new Error(`${label} is required`);
    }
    const count = Number(value);
    if (!Number.isInteger(count) || count < 0 || count > MAX_AGENT_RETENTION_KEEP_RUNS) {
        throw new Error(`${label} must be an integer between 0 and ${MAX_AGENT_RETENTION_KEEP_RUNS}`);
    }
    return count;
}
