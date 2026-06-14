import { errorText, requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';

const RUN_PRUNE_DETAIL_LIMIT = 8;
const MAX_AGENT_RETENTION_KEEP_RUNS = 10000;

export const RunRetentionPanel = {
    data() {
        return {
            loading: false,
            saving: false,
            planning: false,
            error: '',
            retention: null,
            draft: {
                keepRecentTerminalRuns: 100,
                keepFullRecentRuns: 20,
            },
            plan: null,
        };
    },
    computed: {
        busy() {
            return this.loading || this.saving || this.planning;
        },
        draftIsDirty() {
            if (!this.retention) {
                return false;
            }
            try {
                const draft = this.normalizedDraft();
                return draft.keepRecentTerminalRuns !== this.retention.keepRecentTerminalRuns
                    || draft.keepFullRecentRuns !== this.retention.keepFullRecentRuns;
            } catch {
                return true;
            }
        },
        planHasWork() {
            return Number(this.plan?.totalCandidateFileCount || 0) > 0
                || Number(this.plan?.slimCandidateCount || 0) > 0
                || Number(this.plan?.deleteCandidateCount || 0) > 0;
        },
        planStats() {
            const plan = this.plan;
            if (!plan) {
                return [];
            }
            return [
                {
                    key: 'full',
                    icon: 'fa-box-archive',
                    label: tr('runRetentionFullKept'),
                    value: this.formatCount(plan.fullRetainedRunCount),
                    tone: 'full',
                },
                {
                    key: 'core',
                    icon: 'fa-scroll',
                    label: tr('runRetentionCoreKept'),
                    value: this.formatCount(plan.coreRetainedRunCount),
                    tone: 'core',
                },
                {
                    key: 'slim',
                    icon: 'fa-compress',
                    label: tr('runRetentionSlimCandidates'),
                    value: this.formatBytes(plan.totalSlimByteCount),
                    subvalue: tr('runRetentionRunCount', { count: Number(plan.slimCandidateCount || 0) }),
                    tone: 'slim',
                },
                {
                    key: 'delete',
                    icon: 'fa-trash-can',
                    label: tr('runRetentionDeleteCandidates'),
                    value: this.formatBytes(plan.totalDeleteByteCount),
                    subvalue: tr('runRetentionRunCount', { count: Number(plan.deleteCandidateCount || 0) }),
                    tone: 'delete',
                },
            ];
        },
        candidatePreview() {
            return Array.isArray(this.plan?.candidates) ? this.plan.candidates : [];
        },
        blockedPreview() {
            return Array.isArray(this.plan?.blockedRuns) ? this.plan.blockedRuns : [];
        },
    },
    async mounted() {
        await this.loadRetentionSettings();
    },
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        retentionApi() {
            const api = requireHostApi('agent').retention;
            if (typeof api?.readSettings !== 'function'
                || typeof api.updateSettings !== 'function'
                || typeof api.planPrune !== 'function') {
                throw new Error(tr('hostAgentRetentionApiUnavailable'));
            }
            return api;
        },
        async loadRetentionSettings() {
            this.loading = true;
            this.error = '';
            try {
                const retention = this.retentionApi();
                this.applyRetention(await retention.readSettings());
            } catch (error) {
                this.error = errorText(error);
            } finally {
                this.loading = false;
            }
        },
        async saveRetentionSettings() {
            this.saving = true;
            this.error = '';
            try {
                const retention = this.retentionApi();
                const updated = await retention.updateSettings(this.normalizedDraft());
                this.applyRetention(updated);
                window.toastr?.success?.(tr('runRetentionSaved'), tr('agentSystem'));
            } catch (error) {
                this.error = errorText(error);
            } finally {
                this.saving = false;
            }
        },
        async analyzePrune() {
            this.planning = true;
            this.error = '';
            try {
                const retention = this.retentionApi();
                const plan = await retention.planPrune({
                    retention: this.normalizedDraft(),
                    detailLimit: RUN_PRUNE_DETAIL_LIMIT,
                });
                this.plan = normalizePrunePlan(plan);
            } catch (error) {
                this.error = errorText(error);
                this.plan = null;
            } finally {
                this.planning = false;
            }
        },
        applyRetention(value) {
            const retention = normalizeRetentionSettings(value);
            this.retention = retention;
            this.draft = { ...retention };
            this.plan = null;
        },
        setDraftValue(key, event) {
            this.draft = {
                ...this.draft,
                [key]: event?.target?.value,
            };
            this.plan = null;
        },
        normalizedDraft() {
            return normalizeRetentionSettings(this.draft);
        },
        formatCount(value) {
            return String(Number(value || 0));
        },
        formatFiles(value) {
            return tr('fileCount', { count: Number(value || 0) });
        },
        formatBytes(value) {
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
            return `${size.toFixed(precision)} ${units[unitIndex]}`;
        },
        actionLabel(action) {
            switch (String(action || '')) {
                case 'slim_heavy_artifacts':
                    return tr('runRetentionActionSlim');
                case 'delete_run':
                    return tr('runRetentionActionDelete');
                default:
                    return String(action || tr('unknownError'));
            }
        },
        actionIcon(action) {
            return String(action || '') === 'delete_run' ? 'fa-trash-can' : 'fa-compress';
        },
        reasonLabel(reason) {
            switch (String(reason || '')) {
                case 'outside_full_retention_window':
                    return tr('runRetentionReasonOutsideFull');
                case 'outside_history_retention_window':
                    return tr('runRetentionReasonOutsideHistory');
                default:
                    return String(reason || '');
            }
        },
        blockReasonLabel(reason) {
            switch (String(reason || '')) {
                case 'active_run':
                    return tr('runRetentionBlockActive');
                case 'missing_terminal_event':
                    return tr('runRetentionBlockMissingTerminal');
                case 'invalid_journal':
                    return tr('runRetentionBlockInvalidJournal');
                case 'invalid_storage':
                    return tr('runRetentionBlockInvalidStorage');
                default:
                    return String(reason || '');
            }
        },
        runTitle(run) {
            const ref = run?.chatRef || {};
            if (ref.kind === 'character') {
                const characterId = String(ref.characterId || '').trim();
                const fileName = stripChatFileName(ref.fileName);
                return characterId || fileName || tr('runHistoryUnknownChat');
            }
            if (ref.kind === 'group' && ref.chatId) {
                return tr('runHistoryGroupTitle', { id: ref.chatId });
            }
            return run?.stableChatId ? this.shortValue(run.stableChatId) : tr('runHistoryUnknownChat');
        },
        shortValue(value) {
            const text = String(value || '').trim();
            return text.length <= 14 ? text : `${text.slice(0, 10)}...`;
        },
    },
    template: `
        <section class="ttas-retention-panel">
            <header class="ttas-retention-header">
                <div class="ttas-runs-title">
                    <div class="ttas-eyebrow">{{ tr('runRetentionStorage') }}</div>
                    <h4>{{ tr('runRetention') }}</h4>
                </div>
                <div class="ttas-retention-actions">
                    <button type="button" class="menu_button menu_button_icon" :disabled="busy" @click="loadRetentionSettings">
                        <i class="fa-solid" :class="loading ? 'fa-spinner fa-spin' : 'fa-rotate-right'"></i>
                        <span>{{ tr('refresh') }}</span>
                    </button>
                    <button type="button" class="menu_button menu_button_icon" :disabled="busy || !draftIsDirty" @click="saveRetentionSettings">
                        <i class="fa-solid" :class="saving ? 'fa-spinner fa-spin' : 'fa-floppy-disk'"></i>
                        <span>{{ tr('save') }}</span>
                    </button>
                    <button type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="busy" @click="analyzePrune">
                        <i class="fa-solid" :class="planning ? 'fa-spinner fa-spin' : 'fa-broom'"></i>
                        <span>{{ tr('runRetentionAnalyze') }}</span>
                    </button>
                </div>
            </header>

            <div class="ttas-retention-controls">
                <label class="ttas-field">
                    <span>{{ tr('runRetentionKeepHistory') }}</span>
                    <input class="text_pole" type="number" min="0" :max="10000" step="1" :value="draft.keepRecentTerminalRuns" @input="setDraftValue('keepRecentTerminalRuns', $event)" />
                </label>
                <label class="ttas-field">
                    <span>{{ tr('runRetentionKeepFull') }}</span>
                    <input class="text_pole" type="number" min="0" :max="10000" step="1" :value="draft.keepFullRecentRuns" @input="setDraftValue('keepFullRecentRuns', $event)" />
                </label>
            </div>

            <div class="ttas-retention-band">
                <span>
                    <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
                    {{ tr('runRetentionFullSummary', { count: draft.keepFullRecentRuns }) }}
                </span>
                <span>
                    <i class="fa-solid fa-scroll" aria-hidden="true"></i>
                    {{ tr('runRetentionCoreSummary', { count: draft.keepRecentTerminalRuns }) }}
                </span>
            </div>

            <div v-if="error" class="ttas-error ttas-retention-error">
                <i class="fa-solid fa-triangle-exclamation"></i>
                <pre>{{ error }}</pre>
            </div>

            <div v-if="plan" class="ttas-retention-plan">
                <div class="ttas-retention-stat-grid">
                    <div v-for="stat in planStats" :key="stat.key" class="ttas-retention-stat" :data-ttas-tone="stat.tone">
                        <i class="fa-solid" :class="stat.icon" aria-hidden="true"></i>
                        <span>{{ stat.label }}</span>
                        <strong>{{ stat.value }}</strong>
                        <small v-if="stat.subvalue">{{ stat.subvalue }}</small>
                    </div>
                </div>

                <div v-if="planHasWork" class="ttas-retention-plan-summary">
                    <i class="fa-solid fa-database" aria-hidden="true"></i>
                    <strong>{{ formatBytes(plan.totalCandidateByteCount) }}</strong>
                    <span>{{ formatFiles(plan.totalCandidateFileCount) }}</span>
                    <em>{{ tr('runRetentionDryRunOnly') }}</em>
                </div>
                <div v-else class="ttas-retention-plan-empty">
                    <i class="fa-solid fa-circle-check" aria-hidden="true"></i>
                    <span>{{ tr('runRetentionNothingToClean') }}</span>
                </div>

                <ol v-if="candidatePreview.length > 0" class="ttas-retention-preview-list">
                    <li v-for="candidate in candidatePreview" :key="candidate.runId">
                        <span class="ttas-retention-preview-icon" :data-ttas-action="candidate.action">
                            <i class="fa-solid" :class="actionIcon(candidate.action)" aria-hidden="true"></i>
                        </span>
                        <span class="ttas-retention-preview-main">
                            <strong>{{ actionLabel(candidate.action) }}</strong>
                            <small>{{ runTitle(candidate) }} &middot; {{ shortValue(candidate.runId) }} &middot; {{ reasonLabel(candidate.reason) }}</small>
                        </span>
                        <span class="ttas-retention-preview-meta">
                            <strong>{{ formatBytes(candidate.byteCount) }}</strong>
                            <small>{{ formatFiles(candidate.fileCount) }}</small>
                        </span>
                    </li>
                </ol>

                <div v-if="plan.candidateDetailsTruncated" class="ttas-retention-note">
                    <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
                    <span>{{ tr('runRetentionCandidatesTruncated') }}</span>
                </div>

                <ol v-if="blockedPreview.length > 0" class="ttas-retention-preview-list ttas-retention-blocked-list">
                    <li v-for="blocked in blockedPreview" :key="blocked.runId">
                        <span class="ttas-retention-preview-icon" data-ttas-action="blocked">
                            <i class="fa-solid fa-ban" aria-hidden="true"></i>
                        </span>
                        <span class="ttas-retention-preview-main">
                            <strong>{{ blockReasonLabel(blocked.blockReason) }}</strong>
                            <small>{{ runTitle(blocked) }} &middot; {{ shortValue(blocked.runId) }}</small>
                        </span>
                        <span class="ttas-retention-preview-meta">
                            <strong>{{ actionLabel(blocked.action) }}</strong>
                            <small>{{ blocked.message || reasonLabel(blocked.reason) }}</small>
                        </span>
                    </li>
                </ol>
            </div>
        </section>
    `,
};

function normalizeRetentionSettings(value) {
    if (!plainObject(value)) {
        throw new Error('agent.retention_settings_invalid: settings must be an object');
    }
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
        keepRecentTerminalRuns,
        keepFullRecentRuns,
    };
}

function normalizeRetentionCount(value, label) {
    if (value == null || value === '') {
        throw new Error(`${label} is required`);
    }
    const count = Number(value);
    if (!Number.isInteger(count) || count < 0 || count > MAX_AGENT_RETENTION_KEEP_RUNS) {
        throw new Error(`${label} must be an integer between 0 and ${MAX_AGENT_RETENTION_KEEP_RUNS}`);
    }
    return count;
}

function normalizePrunePlan(value) {
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
    return value;
}

function stripChatFileName(value) {
    return String(value || '')
        .trim()
        .replace(/\.(jsonl?|chat)$/i, '');
}

function plainObject(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }
    const prototype = Object.getPrototypeOf(value);
    return prototype === Object.prototype || prototype === null;
}
