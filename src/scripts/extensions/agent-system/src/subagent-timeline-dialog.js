import { errorText, requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    buildEventDetailTargets,
    hasModelTurnNarration,
    isDisplayableRunEvent,
    timelineItemsFromEvents,
} from './run-event-presenter.js';
import {
    RunTimelineDetailPane,
    RunTimelineEventList,
} from './run-timeline-components.js';
import { createTimelineDetailState } from './run-timeline-detail-state.js';
import {
    subAgentStatusLabel,
    subAgentTaskStyle,
    timelineItemTitle,
} from './run-timeline-display.js';
import { createRunTimelineSession } from './run-timeline-session.js';
import { virtualizeTimelineItems } from './run-timeline-virtual-list.js';

function isHtmlDialogElement(value) {
    return typeof HTMLDialogElement !== 'undefined' && value instanceof HTMLDialogElement;
}

export const SubAgentTimelineDialog = {
    components: {
        RunTimelineDetailPane,
        RunTimelineEventList,
    },
    emits: ['action'],
    data() {
        return {
            dialogOpen: false,
            runId: '',
            invocationId: '',
            task: null,
            readOnly: false,
            timelineSession: createRunTimelineSession(),
            selectedSeq: null,
            detail: createTimelineDetailState(),
            timelineScrollTop: 0,
            timelineViewportHeight: 1,
        };
    },
    computed: {
        loadingHistory() {
            return this.timelineSession.loading;
        },
        loadingOlderHistory() {
            return this.timelineSession.loadingOlder;
        },
        taskStyle() {
            return this.task ? subAgentTaskStyle(this.task) : {};
        },
        dialogTitle() {
            return this.task?.displayName || tr('subAgent');
        },
        dialogSubtitle() {
            const task = this.task;
            if (!task) {
                return '';
            }
            return [subAgentStatusLabel(task.status), task.workspaceKey].filter(Boolean).join(' | ');
        },
        displayItems() {
            if (!this.invocationId) {
                return [];
            }
            return timelineItemsFromEvents(this.timelineSession.events, {
                invocationId: this.invocationId,
            });
        },
        virtualDisplayItems() {
            return virtualizeTimelineItems(
                this.displayItems,
                this.timelineScrollTop,
                this.timelineViewportHeight,
            );
        },
        selectedDisplaySeq() {
            return this.selectedItem?.seq ?? null;
        },
        selectedItem() {
            if (this.selectedSeq != null) {
                const selected = this.displayItems.find((item) => item.seq === this.selectedSeq);
                if (selected) {
                    return selected;
                }
            }
            return this.displayItems[this.displayItems.length - 1] || null;
        },
        detailTitle() {
            return this.selectedItem ? timelineItemTitle(this.selectedItem) : tr('timelineDetails');
        },
        detailTargets() {
            if (!this.selectedItem) {
                return [];
            }
            return buildEventDetailTargets(this.selectedItem, this.timelineSession.events);
        },
        navItems() {
            return this.displayItems.slice(-20);
        },
    },
    watch: {
        selectedSeq() {
            if (this.dialogOpen) {
                void this.loadDetails();
            }
        },
    },
    unmounted() {
        const dialog = this.$refs.dialog;
        if (isHtmlDialogElement(dialog) && dialog.open) {
            dialog.close();
        }
    },
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        readAgentEvents(input) {
            return requireHostApi('agent').readEvents(input);
        },
        open({ runId, invocationId, task = null, readOnly = false } = {}) {
            const normalizedRunId = String(runId || '').trim();
            const normalizedInvocationId = String(invocationId || '').trim();
            if (!normalizedRunId) {
                throw new Error('Agent run id is required.');
            }
            if (!normalizedInvocationId) {
                throw new Error('SubAgent invocationId is required.');
            }
            if (typeof HTMLDialogElement === 'undefined') {
                throw new Error(tr('subAgentDialogUnsupported'));
            }

            this.runId = normalizedRunId;
            this.invocationId = normalizedInvocationId;
            this.task = task;
            this.readOnly = Boolean(readOnly);
            this.dialogOpen = true;
            this.selectedSeq = null;
            this.timelineScrollTop = 0;
            this.timelineViewportHeight = 1;
            this.detail.reset();
            this.timelineSession.reset({
                runId: normalizedRunId,
                invocationId: normalizedInvocationId,
            });

            void this.loadHistory();
            this.$nextTick(() => {
                const dialog = this.$refs.dialog;
                if (!isHtmlDialogElement(dialog) || typeof dialog.showModal !== 'function') {
                    throw new Error(tr('subAgentDialogUnsupported'));
                }
                if (!dialog.open) {
                    dialog.showModal();
                }
                this.measureTimelineViewport();
            });
        },
        reset() {
            const dialog = this.$refs.dialog;
            if (isHtmlDialogElement(dialog) && dialog.open) {
                dialog.close();
                return;
            }
            this.onDialogClosed();
        },
        close() {
            const dialog = this.$refs.dialog;
            if (isHtmlDialogElement(dialog) && dialog.open) {
                dialog.close();
                return;
            }
            this.onDialogClosed();
        },
        onDialogClosed() {
            this.dialogOpen = false;
            this.runId = '';
            this.invocationId = '';
            this.task = null;
            this.readOnly = false;
            this.selectedSeq = null;
            this.timelineScrollTop = 0;
            this.timelineViewportHeight = 1;
            this.timelineSession.reset();
            this.detail.reset();
        },
        async loadHistory() {
            const runId = this.runId;
            const invocationId = this.invocationId;
            if (!runId || !invocationId) {
                return;
            }

            try {
                const applied = await this.timelineSession.loadInitial(this.readAgentEvents);
                if (applied && this.runId === runId && this.invocationId === invocationId) {
                    this.$nextTick(() => {
                        this.measureTimelineViewport();
                        this.stickTimelineToBottom();
                        if (this.dialogOpen) {
                            void this.loadDetails();
                        }
                    });
                }
            } catch (error) {
                console.error('[AgentSystem] Failed to load SubAgent events', error);
                window.toastr?.error?.(errorText(error));
            }
        },
        async loadOlderHistory() {
            if (!this.runId || !this.invocationId) {
                return;
            }

            const anchor = this.$refs.timelineList?.captureScrollAnchor?.();
            try {
                const applied = await this.timelineSession.loadOlder(this.readAgentEvents);
                if (applied) {
                    this.$nextTick(() => {
                        this.$refs.timelineList?.restoreScrollAnchor?.(anchor);
                    });
                }
            } catch (error) {
                console.error('[AgentSystem] Failed to load older SubAgent events', error);
                window.toastr?.error?.(errorText(error));
            }
        },
        receiveEvents(events) {
            if (!Array.isArray(events)) {
                throw new Error('agent.timeline_events_invalid: events must be an array');
            }
            if (!this.dialogOpen) {
                return;
            }
            const shouldStick = this.isTimelineNearBottom();
            let shouldRefreshDetails = false;
            for (const event of events) {
                const added = this.timelineSession.receiveEvent(event);
                shouldRefreshDetails = shouldRefreshDetails
                    || (added && this.selectedSeq == null && (isDisplayableRunEvent(event) || hasModelTurnNarration(event)));
            }
            if (shouldRefreshDetails) {
                void this.loadDetails();
            }
            if (shouldStick) {
                this.$nextTick(() => this.stickTimelineToBottom());
            }
        },
        receiveEvent(event, options = {}) {
            if (!this.dialogOpen) {
                return;
            }
            const shouldStick = !options.skipStick && this.isTimelineNearBottom();
            if (!this.timelineSession.receiveEvent(event)) {
                return;
            }
            if (!options.skipDetail
                && this.selectedSeq == null
                && (isDisplayableRunEvent(event) || hasModelTurnNarration(event))) {
                void this.loadDetails();
            }
            if (shouldStick) {
                this.$nextTick(() => this.stickTimelineToBottom());
            }
        },
        selectItem(item) {
            this.selectedSeq = item.seq;
        },
        onTimelineViewport(viewport) {
            this.timelineScrollTop = viewport.scrollTop;
            this.timelineViewportHeight = viewport.viewportHeight;
        },
        measureTimelineViewport() {
            this.$refs.timelineList?.measureViewport?.();
        },
        stickTimelineToBottom() {
            this.$refs.timelineList?.scrollToBottom?.();
        },
        isTimelineNearBottom() {
            return this.$refs.timelineList?.isNearBottom?.() ?? true;
        },
        async loadDetails() {
            const item = this.selectedItem;
            if (!item || !this.runId) {
                this.detail.reset();
                return;
            }
            await this.detail.load({
                runId: this.runId,
                targets: this.detailTargets,
                readOnly: this.readOnly,
            });
        },
    },
    template: `
        <dialog
            ref="dialog"
            class="ttas-dialog ttas-subagent-dialog"
            data-tt-mobile-surface="fullscreen-window"
            @cancel.prevent="close()"
            @close="onDialogClosed"
        >
            <div class="ttas-subagent-panel">
                <header class="ttas-subagent-titlebar">
                    <div class="ttas-subagent-title">
                        <span
                            class="ttas-subagent-title-dot"
                            :style="taskStyle"
                            aria-hidden="true"
                        ></span>
                        <div>
                            <strong>{{ dialogTitle }}</strong>
                            <small>{{ dialogSubtitle }}</small>
                        </div>
                    </div>
                    <button
                        type="button"
                        class="menu_button menu_button_icon ttas-run-icon-button"
                        :title="tr('close')"
                        :aria-label="tr('close')"
                        @click="close"
                    >
                        <i class="fa-solid fa-xmark"></i>
                    </button>
                </header>
                <div class="ttas-subagent-body">
                    <RunTimelineEventList
                        ref="timelineList"
                        :aria-label="tr('timelineSubAgentTimeline')"
                        surface-class="ttas-subagent-timeline"
                        list-class="ttas-subagent-events"
                        :loading="loadingHistory"
                        :loading-older="loadingOlderHistory"
                        :empty-text="tr('timelineNoEvents')"
                        :items="displayItems"
                        :virtual-items="virtualDisplayItems"
                        :selected-seq="selectedDisplaySeq"
                        :latest-seq="null"
                        :active-seq="null"
                        item-key-prefix="subagent-"
                        :mark-latest="false"
                        @select="selectItem"
                        @top-reached="loadOlderHistory"
                        @viewport="onTimelineViewport"
                    />
                    <RunTimelineDetailPane
                        root-class="ttas-subagent-detail"
                        :aria-label="tr('timelineDetails')"
                        :title="detailTitle"
                        :type="selectedItem ? selectedItem.type : ''"
                        :nav-items="navItems"
                        :selected-seq="selectedDisplaySeq"
                        :loading="detail.loading"
                        :error="detail.error"
                        :sections="detail.sections"
                        @select-nav="selectItem"
                        @action="$emit('action', $event)"
                    />
                </div>
            </div>
        </dialog>
    `,
};
