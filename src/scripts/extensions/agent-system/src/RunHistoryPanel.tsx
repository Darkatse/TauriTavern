import { useSyncExternalStore } from 'react';

import { RunRetentionPanel } from './RunRetentionPanel';
import type { RunHistoryController } from './RunHistoryController';
import type { RunRetentionController } from './RunRetentionController';
import type { Tr } from './AgentSystemPanelContract';

export type RunHistoryPanelProps = {
    controller: RunHistoryController;
    retention: RunRetentionController;
    tr: Tr;
};

function stripChatFileName(value: string): string {
    return value
        .trim()
        .replace(/\.(jsonl?|chat)$/i, '');
}

function shortValue(value: string): string {
    const text = value.trim();
    if (text.length <= 14) {
        return text;
    }
    return `${text.slice(0, 10)}...`;
}

function characterChatTitle(ref: { characterId: string; fileName: string }, tr: Tr): string {
    const characterId = ref.characterId.trim();
    const fileName = stripChatFileName(ref.fileName);
    if (characterId && fileName && characterId !== fileName) {
        return `${characterId} / ${fileName}`;
    }
    return characterId || fileName || tr('runHistoryUnknownChat');
}

function runTitle(run: TauriTavernAgentRunSummary, tr: Tr): string {
    const ref = run.chatRef;
    if (ref.kind === 'character') {
        return characterChatTitle(ref, tr);
    }
    if (ref.kind === 'group') {
        return ref.chatId
            ? tr('runHistoryGroupTitle', { id: ref.chatId })
            : tr('runHistoryUnknownChat');
    }
    return run.stableChatId ? shortValue(run.stableChatId) : tr('runHistoryUnknownChat');
}

function generationLabel(value: string, tr: Tr): string {
    const generationType = value.trim();
    if (!generationType) {
        return tr('runHistoryGenerationUnknown');
    }
    return generationType;
}

function commitLabel(run: TauriTavernAgentRunSummary, tr: Tr): string {
    const messageIndex = run.committedMessage?.messageIndex;
    if (typeof messageIndex === 'number' && Number.isInteger(messageIndex) && messageIndex >= 0) {
        return tr('runHistoryCommittedFloor', { index: messageIndex + 1 });
    }
    const commitCount = Number(run.commitCount || 0);
    if (commitCount > 0) {
        return tr('runHistoryCommittedNoFloor', { count: commitCount });
    }
    return tr('runHistoryNoCommit');
}

function runSubtitle(run: TauriTavernAgentRunSummary, tr: Tr): string {
    return [
        generationLabel(run.generationType, tr),
        run.profileId ? tr('runHistoryProfile', { id: run.profileId }) : '',
        commitLabel(run, tr),
    ].filter(Boolean).join(' · ');
}

function chatKindLabel(run: TauriTavernAgentRunSummary, tr: Tr): string {
    switch (run.chatRef?.kind) {
        case 'character':
            return tr('runHistoryCharacterChat');
        case 'group':
            return tr('runHistoryGroupChat');
        default:
            return tr('runHistoryUnknownChat');
    }
}

function statusLabel(status: string, tr: Tr): string {
    switch (status) {
        case 'completed':
            return tr('timelineStatusCompleted');
        case 'partial_success':
            return tr('timelinePartialSuccessMessage');
        case 'cancelled':
            return tr('timelineStatusCancelled');
        case 'failed':
            return tr('timelineStatusFailed');
        default:
            return status;
    }
}

function statusTone(status: string): string {
    switch (status) {
        case 'completed':
            return 'completed';
        case 'partial_success':
            return 'partial';
        case 'cancelled':
            return 'cancelled';
        case 'failed':
            return 'failed';
        default:
            return 'neutral';
    }
}

function runTime(run: TauriTavernAgentRunSummary): string {
    const value = run.terminalAt || run.updatedAt || run.createdAt;
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
        return '';
    }
    return date.toLocaleString([], {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
    });
}

export function RunHistoryPanel({ controller, retention, tr }: RunHistoryPanelProps) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const { runs, nextCursor, loading, loadingMore, filter, error } = snapshot;
    const emptyText = filter === 'current' ? tr('runHistoryCurrentEmpty') : tr('runHistoryEmpty');

    return (
        <div className="ttas-runs-panel">
            <header className="ttas-runs-header">
                <div className="ttas-runs-title">
                    <div className="ttas-eyebrow">{tr('tauriTavernAgent')}</div>
                    <h4>{tr('runHistory')}</h4>
                </div>
                <div className="ttas-runs-actions">
                    <div className="ttas-segmented-control" aria-label={tr('runHistoryFilter')}>
                        <button
                            type="button"
                            className={`menu_button${filter === 'all' ? ' active' : ''}`}
                            onClick={() => void controller.setFilter('all')}
                        >
                            <i className="fa-solid fa-layer-group"></i>
                            <span>{tr('runHistoryAllChats')}</span>
                        </button>
                        <button
                            type="button"
                            className={`menu_button${filter === 'current' ? ' active' : ''}`}
                            onClick={() => void controller.setFilter('current')}
                        >
                            <i className="fa-solid fa-message"></i>
                            <span>{tr('runHistoryCurrentChat')}</span>
                        </button>
                    </div>
                    <button
                        type="button"
                        className="menu_button menu_button_icon"
                        disabled={loading}
                        onClick={() => void controller.refresh()}
                    >
                        <i className={`fa-solid ${loading ? 'fa-spinner fa-spin' : 'fa-rotate-right'}`}></i>
                        <span>{tr('refresh')}</span>
                    </button>
                </div>
            </header>

            <RunRetentionPanel controller={retention} tr={tr} onPruned={() => void controller.refresh()} />

            {error && (
                <div className="ttas-error">
                    <i className="fa-solid fa-triangle-exclamation"></i>
                    <pre>{error}</pre>
                </div>
            )}

            {loading && runs.length === 0 ? (
                <div className="ttas-run-history-loading">
                    <i className="fa-solid fa-spinner fa-spin"></i>
                    <span>{tr('runHistoryLoading')}</span>
                </div>
            ) : runs.length === 0 ? (
                <div className="ttas-empty ttas-run-history-empty">
                    <i className="fa-solid fa-clock-rotate-left"></i>
                    <span>{emptyText}</span>
                </div>
            ) : (
                <ol className="ttas-run-history-list">
                    {runs.map((run) => {
                        const time = runTime(run);
                        return (
                            <li
                                key={run.runId}
                                className="ttas-run-history-row"
                                data-ttas-status={statusTone(run.status)}
                            >
                                <button type="button" onClick={() => controller.openRun(run)}>
                                    <span className="ttas-run-history-status" aria-hidden="true">
                                        <i className="fa-solid fa-clock-rotate-left"></i>
                                    </span>
                                    <span className="ttas-run-history-main">
                                        <span className="ttas-run-history-line">
                                            <strong>{runTitle(run, tr)}</strong>
                                            <em>{chatKindLabel(run, tr)}</em>
                                        </span>
                                        <small>{runSubtitle(run, tr)}</small>
                                    </span>
                                    <span className="ttas-run-history-meta">
                                        <span data-ttas-status={statusTone(run.status)}>{statusLabel(run.status, tr)}</span>
                                        {time && <time>{time}</time>}
                                        <code>{shortValue(run.runId)}</code>
                                    </span>
                                    <span className="ttas-run-history-open" title={tr('runHistoryOpenTimeline')} aria-label={tr('runHistoryOpenTimeline')}>
                                        <i className="fa-solid fa-arrow-up-right-from-square"></i>
                                    </span>
                                </button>
                            </li>
                        );
                    })}
                </ol>
            )}

            {runs.length > 0 && (
                <div className="ttas-run-history-footer">
                    <span>{tr('runHistoryShown', { count: runs.length })}</span>
                    <button
                        type="button"
                        className="menu_button menu_button_icon"
                        disabled={!nextCursor || loadingMore}
                        onClick={() => void controller.loadMore()}
                    >
                        <i className={`fa-solid ${loadingMore ? 'fa-spinner fa-spin' : 'fa-chevron-down'}`}></i>
                        <span>{tr('runHistoryLoadMore')}</span>
                    </button>
                </div>
            )}
        </div>
    );
}
