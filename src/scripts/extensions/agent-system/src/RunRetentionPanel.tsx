import { useSyncExternalStore } from 'react';

import {
    formatRetentionBytes,
    formatRetentionCount,
    formatRetentionFiles,
    MAX_AGENT_RETENTION_KEEP_RUNS,
    retentionBusy,
    retentionCanApplyPrune,
    retentionDraftIsDirty,
    retentionPlanHasWork,
    type RunRetentionController,
} from './RunRetentionController';
import type { Tr } from './AgentSystemPanelContract';

export type RunRetentionPanelProps = {
    controller: RunRetentionController;
    tr: Tr;
    // The parent refreshes run history after a successful prune.
    onPruned: (result: TauriTavernAgentRunPruneApplyResult) => void;
};

type PlanStat = {
    key: string;
    icon: string;
    label: string;
    value: string;
    subvalue?: string;
    tone: string;
};

function planStats(plan: TauriTavernAgentRunPrunePlan, tr: Tr): PlanStat[] {
    const fullRetainedRunCount = Number(plan.fullRetainedRunCount || 0);
    const reviewableRunCount = fullRetainedRunCount + Number(plan.coreRetainedRunCount || 0);
    return [
        {
            key: 'full',
            icon: 'fa-box-archive',
            label: tr('runRetentionFullKept'),
            value: formatRetentionCount(fullRetainedRunCount),
            tone: 'full',
        },
        {
            key: 'core',
            icon: 'fa-scroll',
            label: tr('runRetentionCoreKept'),
            value: formatRetentionCount(reviewableRunCount),
            tone: 'core',
        },
        {
            key: 'slim',
            icon: 'fa-compress',
            label: tr('runRetentionSlimCandidates'),
            value: formatRetentionBytes(plan.totalSlimByteCount),
            subvalue: tr('runRetentionRunCount', { count: Number(plan.slimCandidateCount || 0) }),
            tone: 'slim',
        },
        {
            key: 'delete',
            icon: 'fa-trash-can',
            label: tr('runRetentionDeleteCandidates'),
            value: formatRetentionBytes(plan.totalDeleteByteCount),
            subvalue: tr('runRetentionRunCount', { count: Number(plan.deleteCandidateCount || 0) }),
            tone: 'delete',
        },
    ];
}

function actionLabel(action: string, tr: Tr): string {
    switch (action) {
        case 'slim_heavy_artifacts':
            return tr('runRetentionActionSlim');
        case 'delete_run':
            return tr('runRetentionActionDelete');
        default:
            return action || tr('unknownError');
    }
}

function actionIcon(action: string): string {
    return action === 'delete_run' ? 'fa-trash-can' : 'fa-compress';
}

function reasonLabel(reason: string, tr: Tr): string {
    switch (reason) {
        case 'outside_full_retention_window':
            return tr('runRetentionReasonOutsideFull');
        case 'outside_history_retention_window':
            return tr('runRetentionReasonOutsideHistory');
        default:
            return reason;
    }
}

function blockReasonLabel(reason: string, tr: Tr): string {
    switch (reason) {
        case 'active_run':
            return tr('runRetentionBlockActive');
        case 'missing_terminal_event':
            return tr('runRetentionBlockMissingTerminal');
        case 'invalid_journal':
            return tr('runRetentionBlockInvalidJournal');
        case 'invalid_storage':
            return tr('runRetentionBlockInvalidStorage');
        default:
            return reason;
    }
}

function stripChatFileName(value: string): string {
    return value
        .trim()
        .replace(/\.(jsonl?|chat)$/i, '');
}

function shortValue(value: string): string {
    const text = value.trim();
    return text.length <= 14 ? text : `${text.slice(0, 10)}...`;
}

function runTitle(run: TauriTavernAgentRunPruneCandidate, tr: Tr): string {
    const ref = run.chatRef;
    if (ref.kind === 'character') {
        const characterId = ref.characterId.trim();
        const fileName = stripChatFileName(ref.fileName);
        return characterId || fileName || tr('runHistoryUnknownChat');
    }
    if (ref.kind === 'group' && ref.chatId) {
        return tr('runHistoryGroupTitle', { id: ref.chatId });
    }
    return run.stableChatId ? shortValue(run.stableChatId) : tr('runHistoryUnknownChat');
}

export function RunRetentionPanel({ controller, tr, onPruned }: RunRetentionPanelProps) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const { draft, plan, loading, saving, planning, applying, error } = snapshot;
    const busy = retentionBusy(snapshot);
    const draftIsDirty = retentionDraftIsDirty(snapshot);
    const planHasWork = retentionPlanHasWork(plan);
    const canApplyPrune = retentionCanApplyPrune(snapshot);
    const candidatePreview = plan?.candidates ?? [];
    const blockedPreview = plan?.blockedRuns ?? [];

    async function handleApplyPrune(): Promise<void> {
        const result = await controller.applyPrune();
        if (result) {
            onPruned(result);
        }
    }

    const autoPruneCopy = (
        <span className="ttas-retention-auto-copy">
            <strong>{tr('runRetentionAutoPrune')}</strong>
            <small>{tr('runRetentionAutoPruneHint')}</small>
        </span>
    );

    return (
        <section className="ttas-retention-panel">
            <header className="ttas-retention-header">
                <div className="ttas-runs-title">
                    <div className="ttas-eyebrow">{tr('runRetentionStorage')}</div>
                    <h4>{tr('runRetention')}</h4>
                </div>
                <div className="ttas-retention-actions">
                    <button type="button" className="menu_button menu_button_icon" disabled={busy} onClick={() => void controller.refresh()}>
                        <i className={`fa-solid ${loading ? 'fa-spinner fa-spin' : 'fa-rotate-right'}`}></i>
                        <span>{tr('refresh')}</span>
                    </button>
                    <button type="button" className="menu_button menu_button_icon" disabled={busy || !draftIsDirty} onClick={() => void controller.save()}>
                        <i className={`fa-solid ${saving ? 'fa-spinner fa-spin' : 'fa-floppy-disk'}`}></i>
                        <span>{tr('save')}</span>
                    </button>
                    <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={busy} onClick={() => void controller.analyze()}>
                        <i className={`fa-solid ${planning ? 'fa-spinner fa-spin' : 'fa-broom'}`}></i>
                        <span>{tr('runRetentionAnalyze')}</span>
                    </button>
                    <button type="button" className="menu_button menu_button_icon ttas-danger-button ttas-retention-apply-button" disabled={!canApplyPrune} onClick={() => void handleApplyPrune()}>
                        <i className={`fa-solid ${applying ? 'fa-spinner fa-spin' : 'fa-trash-can'}`}></i>
                        <span>{applying ? tr('runRetentionApplying') : tr('runRetentionApply')}</span>
                    </button>
                </div>
            </header>

            <div className="ttas-retention-controls">
                <div className="ttas-retention-automation" data-ttas-enabled={draft.autoPruneEnabled ? 'true' : 'false'}>
                    <label className="ttas-retention-auto-toggle">
                        <input type="checkbox" checked={draft.autoPruneEnabled} onChange={(event) => controller.setAutoPruneEnabled(event.target.checked)} />
                        <span className="ttas-retention-auto-track" aria-hidden="true">
                            <span></span>
                        </span>
                        {autoPruneCopy}
                    </label>
                    <span className="ttas-retention-auto-state">
                        {draft.autoPruneEnabled ? tr('runRetentionAutoPruneOn') : tr('runRetentionAutoPruneOff')}
                    </span>
                </div>
                <label className="ttas-field">
                    <span>{tr('runRetentionKeepHistory')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="0"
                        max={MAX_AGENT_RETENTION_KEEP_RUNS}
                        step="1"
                        value={draft.keepRecentTerminalRuns}
                        onChange={(event) => controller.setKeepRecentTerminalRuns(event.target.value)}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('runRetentionKeepFull')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="0"
                        max={MAX_AGENT_RETENTION_KEEP_RUNS}
                        step="1"
                        value={draft.keepFullRecentRuns}
                        onChange={(event) => controller.setKeepFullRecentRuns(event.target.value)}
                    />
                </label>
            </div>

            <div className="ttas-retention-band">
                <span>
                    <i className="fa-solid fa-box-archive" aria-hidden="true"></i>
                    {tr('runRetentionFullSummary', { count: draft.keepFullRecentRuns })}
                </span>
                <span>
                    <i className="fa-solid fa-scroll" aria-hidden="true"></i>
                    {tr('runRetentionCoreSummary', { count: draft.keepRecentTerminalRuns })}
                </span>
                <span className="ttas-retention-auto-pill" data-ttas-enabled={draft.autoPruneEnabled ? 'true' : 'false'}>
                    <i className={`fa-solid ${draft.autoPruneEnabled ? 'fa-clock-rotate-left' : 'fa-pause'}`} aria-hidden="true"></i>
                    {draft.autoPruneEnabled ? tr('runRetentionAutoSummaryOn') : tr('runRetentionAutoSummaryOff')}
                </span>
            </div>

            {error && (
                <div className="ttas-error ttas-retention-error">
                    <i className="fa-solid fa-triangle-exclamation"></i>
                    <pre>{error}</pre>
                </div>
            )}

            {plan && (
                <div className="ttas-retention-plan">
                    <div className="ttas-retention-stat-grid">
                        {planStats(plan, tr).map((stat) => (
                            <div key={stat.key} className="ttas-retention-stat" data-ttas-tone={stat.tone}>
                                <i className={`fa-solid ${stat.icon}`} aria-hidden="true"></i>
                                <span>{stat.label}</span>
                                <strong>{stat.value}</strong>
                                {stat.subvalue && <small>{stat.subvalue}</small>}
                            </div>
                        ))}
                    </div>

                    {planHasWork ? (
                        <div className="ttas-retention-plan-summary">
                            <i className="fa-solid fa-database" aria-hidden="true"></i>
                            <strong>{formatRetentionBytes(plan.totalCandidateByteCount)}</strong>
                            <span>{formatRetentionFiles(plan.totalCandidateFileCount, tr)}</span>
                            <em>{tr('runRetentionDryRunOnly')}</em>
                        </div>
                    ) : (
                        <div className="ttas-retention-plan-empty">
                            <i className="fa-solid fa-circle-check" aria-hidden="true"></i>
                            <span>{tr('runRetentionNothingToClean')}</span>
                        </div>
                    )}

                    {candidatePreview.length > 0 && (
                        <ol className="ttas-retention-preview-list">
                            {candidatePreview.map((candidate) => (
                                <li key={candidate.runId}>
                                    <span className="ttas-retention-preview-icon" data-ttas-action={candidate.action}>
                                        <i className={`fa-solid ${actionIcon(candidate.action)}`} aria-hidden="true"></i>
                                    </span>
                                    <span className="ttas-retention-preview-main">
                                        <strong>{actionLabel(candidate.action, tr)}</strong>
                                        <small>{runTitle(candidate, tr)} &middot; {shortValue(candidate.runId)} &middot; {reasonLabel(candidate.reason, tr)}</small>
                                    </span>
                                    <span className="ttas-retention-preview-meta">
                                        <strong>{formatRetentionBytes(candidate.byteCount)}</strong>
                                        <small>{formatRetentionFiles(candidate.fileCount, tr)}</small>
                                    </span>
                                </li>
                            ))}
                        </ol>
                    )}

                    {plan.candidateDetailsTruncated && (
                        <div className="ttas-retention-note">
                            <i className="fa-solid fa-ellipsis" aria-hidden="true"></i>
                            <span>{tr('runRetentionCandidatesTruncated')}</span>
                        </div>
                    )}

                    {blockedPreview.length > 0 && (
                        <ol className="ttas-retention-preview-list ttas-retention-blocked-list">
                            {blockedPreview.map((blocked) => (
                                <li key={blocked.runId}>
                                    <span className="ttas-retention-preview-icon" data-ttas-action="blocked">
                                        <i className="fa-solid fa-ban" aria-hidden="true"></i>
                                    </span>
                                    <span className="ttas-retention-preview-main">
                                        <strong>{blockReasonLabel(blocked.blockReason, tr)}</strong>
                                        <small>{runTitle(blocked, tr)} &middot; {shortValue(blocked.runId)}</small>
                                    </span>
                                    <span className="ttas-retention-preview-meta">
                                        <strong>{actionLabel(blocked.action, tr)}</strong>
                                        <small>{blocked.message || reasonLabel(blocked.reason, tr)}</small>
                                    </span>
                                </li>
                            ))}
                        </ol>
                    )}
                </div>
            )}
        </section>
    );
}
