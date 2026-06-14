import { errorText, requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { RunRetentionPanel } from './RunRetentionPanel.js';
import { openAgentRunTimelineDialog } from './run-timeline-panel.js';

const RUN_HISTORY_PAGE_LIMIT = 50;
const TERMINAL_RUN_STATUSES = Object.freeze([
    'completed',
    'partial_success',
    'cancelled',
    'failed',
]);

export const RunHistoryPanel = {
    components: {
        RunRetentionPanel,
    },
    data() {
        return {
            runs: [],
            nextCursor: null,
            loading: false,
            loadingMore: false,
            filter: 'all',
            error: '',
        };
    },
    computed: {
        isCurrentChatFilter() {
            return this.filter === 'current';
        },
        emptyText() {
            return this.isCurrentChatFilter
                ? tr('runHistoryCurrentEmpty')
                : tr('runHistoryEmpty');
        },
    },
    async mounted() {
        await this.refreshRuns();
    },
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        async setFilter(filter) {
            const next = filter === 'current' ? 'current' : 'all';
            if (next === this.filter) {
                return;
            }
            this.filter = next;
            await this.refreshRuns();
        },
        async refreshRuns() {
            this.loading = true;
            this.loadingMore = false;
            this.error = '';
            try {
                const result = normalizeRunHistoryResult(await this.listRuns());
                this.runs = result.runs;
                this.nextCursor = result.nextCursor;
            } catch (error) {
                this.error = errorText(error);
                this.runs = [];
                this.nextCursor = null;
            } finally {
                this.loading = false;
            }
        },
        async loadMoreRuns() {
            if (this.loading || this.loadingMore || !this.nextCursor) {
                return;
            }
            this.loadingMore = true;
            this.error = '';
            try {
                const result = normalizeRunHistoryResult(await this.listRuns({ before: this.nextCursor }));
                this.runs = [...this.runs, ...result.runs];
                this.nextCursor = result.nextCursor;
            } catch (error) {
                this.error = errorText(error);
            } finally {
                this.loadingMore = false;
            }
        },
        async listRuns(extra = {}) {
            const input = {
                statuses: TERMINAL_RUN_STATUSES,
                limit: RUN_HISTORY_PAGE_LIMIT,
                ...extra,
            };

            if (this.isCurrentChatFilter) {
                const currentChat = await this.currentChatRunFilter();
                input.chatRef = currentChat.chatRef;
                input.stableChatId = currentChat.stableChatId;
            }

            return requireHostApi('agent').listRuns(input);
        },
        async currentChatRunFilter() {
            const chat = requireHostApi('chat');
            const chatRef = chat.current.ref();
            if (!plainObject(chatRef)) {
                throw new Error('agent.run_history_current_chat_invalid: current chat ref must be an object');
            }
            const stableChatIdValue = await chat.current.handle().stableId();
            if (typeof stableChatIdValue !== 'string') {
                throw new Error('agent.run_history_current_chat_invalid: stableChatId must be a string');
            }
            const stableChatId = stableChatIdValue.trim();
            if (!stableChatId) {
                throw new Error('agent.run_history_current_chat_invalid: stableChatId is required');
            }
            return { chatRef, stableChatId };
        },
        openRun(run) {
            try {
                openAgentRunTimelineDialog(run);
            } catch (error) {
                console.error('[AgentSystem] Failed to open Agent run timeline', error);
                window.toastr?.error?.(errorText(error), tr('agentSystem'));
            }
        },
        runTitle(run) {
            const ref = run?.chatRef || {};
            if (ref.kind === 'character') {
                return this.characterChatTitle(ref);
            }
            if (ref.kind === 'group') {
                return ref.chatId
                    ? tr('runHistoryGroupTitle', { id: ref.chatId })
                    : tr('runHistoryUnknownChat');
            }
            return run?.stableChatId ? this.shortValue(run.stableChatId) : tr('runHistoryUnknownChat');
        },
        characterChatTitle(ref) {
            const characterId = String(ref.characterId || '').trim();
            const fileName = stripChatFileName(ref.fileName);
            if (characterId && fileName && characterId !== fileName) {
                return `${characterId} / ${fileName}`;
            }
            return characterId || fileName || tr('runHistoryUnknownChat');
        },
        runSubtitle(run) {
            return [
                this.generationLabel(run?.generationType),
                run?.profileId ? tr('runHistoryProfile', { id: run.profileId }) : '',
                this.commitLabel(run),
            ].filter(Boolean).join(' · ');
        },
        chatKindLabel(run) {
            switch (run?.chatRef?.kind) {
                case 'character':
                    return tr('runHistoryCharacterChat');
                case 'group':
                    return tr('runHistoryGroupChat');
                default:
                    return tr('runHistoryUnknownChat');
            }
        },
        commitLabel(run) {
            const messageIndex = run?.committedMessage?.messageIndex;
            if (Number.isInteger(messageIndex) && messageIndex >= 0) {
                return tr('runHistoryCommittedFloor', { index: messageIndex + 1 });
            }
            const commitCount = Number(run?.commitCount || 0);
            if (commitCount > 0) {
                return tr('runHistoryCommittedNoFloor', { count: commitCount });
            }
            return tr('runHistoryNoCommit');
        },
        generationLabel(value) {
            const generationType = String(value || '').trim();
            if (!generationType) {
                return tr('runHistoryGenerationUnknown');
            }
            return generationType;
        },
        statusLabel(status) {
            switch (String(status || '')) {
                case 'completed':
                    return tr('timelineStatusCompleted');
                case 'partial_success':
                    return tr('timelinePartialSuccessMessage');
                case 'cancelled':
                    return tr('timelineStatusCancelled');
                case 'failed':
                    return tr('timelineStatusFailed');
                default:
                    return String(status || '');
            }
        },
        statusTone(status) {
            switch (String(status || '')) {
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
        },
        runTime(run) {
            const value = run?.terminalAt || run?.updatedAt || run?.createdAt;
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
        },
        shortRunId(runId) {
            return this.shortValue(runId);
        },
        shortValue(value) {
            const text = String(value || '').trim();
            if (text.length <= 14) {
                return text;
            }
            return `${text.slice(0, 10)}...`;
        },
    },
    template: `
        <div class="ttas-runs-panel">
            <header class="ttas-runs-header">
                <div class="ttas-runs-title">
                    <div class="ttas-eyebrow">{{ tr('tauriTavernAgent') }}</div>
                    <h4>{{ tr('runHistory') }}</h4>
                </div>
                <div class="ttas-runs-actions">
                    <div class="ttas-segmented-control" :aria-label="tr('runHistoryFilter')">
                        <button type="button" class="menu_button" :class="{ active: filter === 'all' }" @click="setFilter('all')">
                            <i class="fa-solid fa-layer-group"></i>
                            <span>{{ tr('runHistoryAllChats') }}</span>
                        </button>
                        <button type="button" class="menu_button" :class="{ active: filter === 'current' }" @click="setFilter('current')">
                            <i class="fa-solid fa-message"></i>
                            <span>{{ tr('runHistoryCurrentChat') }}</span>
                        </button>
                    </div>
                    <button type="button" class="menu_button menu_button_icon" :disabled="loading" @click="refreshRuns">
                        <i class="fa-solid" :class="loading ? 'fa-spinner fa-spin' : 'fa-rotate-right'"></i>
                        <span>{{ tr('refresh') }}</span>
                    </button>
                </div>
            </header>

            <RunRetentionPanel />

            <div v-if="error" class="ttas-error">
                <i class="fa-solid fa-triangle-exclamation"></i>
                <pre>{{ error }}</pre>
            </div>

            <div v-if="loading && runs.length === 0" class="ttas-run-history-loading">
                <i class="fa-solid fa-spinner fa-spin"></i>
                <span>{{ tr('runHistoryLoading') }}</span>
            </div>
            <div v-else-if="runs.length === 0" class="ttas-empty ttas-run-history-empty">
                <i class="fa-solid fa-clock-rotate-left"></i>
                <span>{{ emptyText }}</span>
            </div>
            <ol v-else class="ttas-run-history-list">
                <li
                    v-for="run in runs"
                    :key="run.runId"
                    class="ttas-run-history-row"
                    :data-ttas-status="statusTone(run.status)"
                >
                    <button type="button" @click="openRun(run)">
                        <span class="ttas-run-history-status" aria-hidden="true">
                            <i class="fa-solid fa-clock-rotate-left"></i>
                        </span>
                        <span class="ttas-run-history-main">
                            <span class="ttas-run-history-line">
                                <strong>{{ runTitle(run) }}</strong>
                                <em>{{ chatKindLabel(run) }}</em>
                            </span>
                            <small>{{ runSubtitle(run) }}</small>
                        </span>
                        <span class="ttas-run-history-meta">
                            <span :data-ttas-status="statusTone(run.status)">{{ statusLabel(run.status) }}</span>
                            <time v-if="runTime(run)">{{ runTime(run) }}</time>
                            <code>{{ shortRunId(run.runId) }}</code>
                        </span>
                        <span class="ttas-run-history-open" :title="tr('runHistoryOpenTimeline')" :aria-label="tr('runHistoryOpenTimeline')">
                            <i class="fa-solid fa-arrow-up-right-from-square"></i>
                        </span>
                    </button>
                </li>
            </ol>

            <div v-if="runs.length > 0" class="ttas-run-history-footer">
                <span>{{ tr('runHistoryShown', { count: runs.length }) }}</span>
                <button type="button" class="menu_button menu_button_icon" :disabled="!nextCursor || loadingMore" @click="loadMoreRuns">
                    <i class="fa-solid" :class="loadingMore ? 'fa-spinner fa-spin' : 'fa-chevron-down'"></i>
                    <span>{{ tr('runHistoryLoadMore') }}</span>
                </button>
            </div>
        </div>
    `,
};

function stripChatFileName(value) {
    return String(value || '')
        .trim()
        .replace(/\.(jsonl?|chat)$/i, '');
}

function normalizeRunHistoryResult(value) {
    if (!plainObject(value)) {
        throw new Error('agent.run_history_result_invalid: result must be an object');
    }
    if (!Array.isArray(value.runs)) {
        throw new Error('agent.run_history_result_invalid: result.runs must be an array');
    }
    if (value.nextCursor != null && !plainObject(value.nextCursor)) {
        throw new Error('agent.run_history_result_invalid: result.nextCursor must be an object or null');
    }
    return {
        runs: value.runs,
        nextCursor: value.nextCursor || null,
    };
}

function plainObject(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }
    const prototype = Object.getPrototypeOf(value);
    return prototype === Object.prototype || prototype === null;
}
