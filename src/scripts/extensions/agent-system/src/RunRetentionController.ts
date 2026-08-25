import { errorText } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import type { Tr } from './AgentSystemPanelContract';

const RUN_PRUNE_DETAIL_LIMIT = 8;
const MAX_AGENT_RETENTION_KEEP_RUNS = 10000;

/**
 * Transient draft: the number inputs hold raw text (string) while typing;
 * normalizeRetentionSettings validates/converts at save/plan/apply time.
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
        const draft = normalizeRetentionSettings(snapshot.draft);
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

    function commit(next: RunRetentionSnapshot): void {
        if (disposed) {
            return;
        }
        snapshot = next;
        for (const listener of listeners) {
            listener();
        }
    }

    function applyRetention(value: unknown): void {
        const retention = normalizeRetentionSettings(value);
        commit({ ...snapshot, retention, draft: { ...retention }, plan: null });
    }

    async function refresh(): Promise<void> {
        commit({ ...snapshot, loading: true, error: '' });
        try {
            applyRetention(await deps.getRetentionApi().readSettings());
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ ...snapshot, error: errorText(error) });
        } finally {
            commit({ ...snapshot, loading: false });
        }
    }

    async function save(): Promise<void> {
        commit({ ...snapshot, saving: true, error: '' });
        try {
            const updated = await deps.getRetentionApi().updateSettings(normalizeRetentionSettings(snapshot.draft));
            if (disposed) {
                return;
            }
            applyRetention(updated);
            deps.notifySuccess(deps.tr('runRetentionSaved'));
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ ...snapshot, error: errorText(error) });
        } finally {
            commit({ ...snapshot, saving: false });
        }
    }

    async function analyze(): Promise<void> {
        commit({ ...snapshot, planning: true, error: '' });
        try {
            const plan = await deps.getRetentionApi().planPrune({
                retention: normalizeRetentionSettings(snapshot.draft),
                detailLimit: RUN_PRUNE_DETAIL_LIMIT,
            });
            if (disposed) {
                return;
            }
            commit({ ...snapshot, plan: normalizePrunePlan(plan) });
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ ...snapshot, error: errorText(error), plan: null });
        } finally {
            commit({ ...snapshot, planning: false });
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

        commit({ ...snapshot, error: '' });
        let confirmed: boolean;
        try {
            confirmed = await deps.confirmAction(deps.tr('runRetentionApplyConfirm', {
                bytes: formatRetentionBytes(plan.totalCandidateByteCount),
                files: formatRetentionFiles(plan.totalCandidateFileCount, deps.tr),
            }));
        } catch (error) {
            if (!disposed) {
                commit({ ...snapshot, error: errorText(error) });
            }
            return null;
        }
        if (!confirmed || disposed) {
            return null;
        }

        commit({ ...snapshot, applying: true });
        try {
            const result = normalizePruneApplyResult(await deps.getRetentionApi().applyPrune({
                retention: normalizeRetentionSettings(snapshot.draft),
                detailLimit: RUN_PRUNE_DETAIL_LIMIT,
            }));
            if (disposed) {
                return null;
            }
            commit({ ...snapshot, plan: result.afterPlan });

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
                commit({ ...snapshot, error: errorText(error) });
            }
            return null;
        } finally {
            commit({ ...snapshot, applying: false });
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
            commit({ ...snapshot, draft: { ...snapshot.draft, autoPruneEnabled: Boolean(enabled) }, plan: null });
        },
        setKeepRecentTerminalRuns(value: string): void {
            commit({ ...snapshot, draft: { ...snapshot.draft, keepRecentTerminalRuns: value }, plan: null });
        },
        setKeepFullRecentRuns(value: string): void {
            commit({ ...snapshot, draft: { ...snapshot.draft, keepFullRecentRuns: value }, plan: null });
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

export function normalizeRetentionSettings(value: unknown): TauriTavernAgentRunRetentionSettings {
    if (!plainObject(value)) {
        throw new Error('agent.retention_settings_invalid: settings must be an object');
    }
    const autoPruneEnabled = normalizeRetentionAutoPrune(
        value.autoPruneEnabled ?? value.auto_prune_enabled ?? false,
        'autoPruneEnabled',
    );
    const keepRecentTerminalRuns = normalizeRetentionCount(
        value.keepRecentTerminalRuns ?? value.keep_recent_terminal_runs,
        'keepRecentTerminalRuns',
    );
    const keepFullRecentRuns = normalizeRetentionCount(
        value.keepFullRecentRuns ?? value.keep_full_recent_runs,
        'keepFullRecentRuns',
    );
    if (keepFullRecentRuns > keepRecentTerminalRuns) {
        throw new Error(tr('runRetentionFullExceedsHistory'));
    }
    return {
        autoPruneEnabled,
        keepRecentTerminalRuns,
        keepFullRecentRuns,
    };
}

function normalizeRetentionAutoPrune(value: unknown, label: string): boolean {
    if (typeof value !== 'boolean') {
        throw new Error(`${label} must be a boolean`);
    }
    return value;
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

function normalizePrunePlan(value: unknown): TauriTavernAgentRunPrunePlan {
    if (!plainObject(value)) {
        throw new Error('agent.run_prune_plan_invalid: plan must be an object');
    }
    if (!Array.isArray(value.candidates)) {
        throw new Error('agent.run_prune_plan_invalid: plan.candidates must be an array');
    }
    if (!Array.isArray(value.blockedRuns)) {
        throw new Error('agent.run_prune_plan_invalid: plan.blockedRuns must be an array');
    }
    normalizeRetentionSettings(value.retention);
    return value as TauriTavernAgentRunPrunePlan;
}

function normalizePruneApplyResult(value: unknown): TauriTavernAgentRunPruneApplyResult {
    if (!plainObject(value)) {
        throw new Error('agent.run_prune_apply_invalid: result must be an object');
    }
    if (!Array.isArray(value.failedRuns)) {
        throw new Error('agent.run_prune_apply_invalid: result.failedRuns must be an array');
    }
    normalizeRetentionSettings(value.retention);
    return {
        ...(value as Omit<TauriTavernAgentRunPruneApplyResult, 'afterPlan'> & { afterPlan: unknown }),
        afterPlan: normalizePrunePlan(value.afterPlan),
    };
}

function plainObject(value: unknown): value is Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }
    const prototype = Object.getPrototypeOf(value) as unknown;
    return prototype === Object.prototype || prototype === null;
}
