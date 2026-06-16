import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { errorText, requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import {
    clampRunTimelineHeightPx,
    heightFromTopEdgeDrag,
    normalizeRunTimelineHeightPx,
    RUN_TIMELINE_KEYBOARD_STEP_PX,
    RUN_TIMELINE_PAGE_STEP_PX,
    runTimelineHeightBounds,
} from './run-timeline-resize.js';
import {
    projectAgentInvocations,
} from './run-invocation-projector.js';
import {
    getActiveAgentRun,
    subscribeAgentRunEvents,
    subscribeAgentRunState,
} from '../../../tauritavern/agent/agent-run-controller.js';
import { retryAgentRunFailure } from '../../../tauritavern/agent/agent-run-retry.js';
import {
    buildEventDetailTargets,
    hasModelTurnNarration,
    isDisplayableRunEvent,
    timelineItemsFromEvents,
} from './run-event-presenter.js';
import {
    RunTimelineDetailPane,
    RunTimelineEventList,
    SubAgentTray,
} from './run-timeline-components.js';
import { createTimelineDetailState } from './run-timeline-detail-state.js';
import {
    shortRunId,
    timelineItemTitle,
} from './run-timeline-display.js';
import { isTimelineProjectionStructuralEvent } from './run-timeline-projection.js';
import { createRunTimelineSession } from './run-timeline-session.js';
import { virtualizeTimelineItems } from './run-timeline-virtual-list.js';
import { SubAgentTimelineDialog } from './subagent-timeline-dialog.js';

const MOUNT_ID = 'ttas_agent_run_timeline_mount';

let historyTimelineDialogCounter = 0;

function createAgentRunTimelineApp(options = {}) {
    const timelineOptions = normalizeTimelineOptions(options);

    return createApp({
        components: {
            RunTimelineDetailPane,
            RunTimelineEventList,
            SubAgentTimelineDialog,
            SubAgentTray,
        },
        data() {
            return {
                timelineMode: timelineOptions.mode,
                rootId: timelineOptions.rootId,
                readOnly: timelineOptions.readOnly,
                requestClose: timelineOptions.requestClose,
                initialRun: timelineOptions.run,
                initialCollapsed: timelineOptions.collapsed,
                settings: {
                    agentModeEnabled: false,
                    runTimelineHeightPx: null,
                },
                currentRun: null,
                activeRun: null,
                timelineSession: createRunTimelineSession({ includeTimelineProjection: true }),
                timelineProjectionRefreshTimer: null,
                collapsed: true,
                detailsOpen: false,
                selectedSeq: null,
                autoStick: true,
                detail: createTimelineDetailState(),
                subAgentTrayExpanded: false,
                panelHeightPx: null,
                resizing: false,
                resizeStartY: 0,
                resizeStartHeightPx: 0,
                resizeBounds: null,
                timelineScrollTop: 0,
                timelineViewportHeight: 1,
                unsubscribeSettings: null,
                unsubscribeRunState: null,
                unsubscribeRunEvents: null,
            };
        },
        computed: {
            isHistoryMode() {
                return this.timelineMode === 'history';
            },
            visible() {
                if (this.isHistoryMode) {
                    return true;
                }
                return Boolean(this.settings.agentModeEnabled);
            },
            canResize() {
                return !this.isHistoryMode;
            },
            events() {
                return this.timelineSession.events;
            },
            timelineProjection() {
                return this.timelineSession.timelineProjection;
            },
            terminalEvent() {
                return this.timelineSession.terminalEvent;
            },
            loadingHistory() {
                return this.timelineSession.loading;
            },
            loadingOlderHistory() {
                return this.timelineSession.loadingOlder;
            },
            isRunning() {
                return Boolean(this.activeRun?.runId && this.currentRun?.runId === this.activeRun.runId);
            },
            terminalType() {
                return this.terminalEvent?.type || '';
            },
            panelStatus() {
                if (this.terminalType === 'run_failed') {
                    return 'failed';
                }
                if (this.terminalType === 'run_cancelled') {
                    return 'cancelled';
                }
                if (this.terminalType === 'run_partial_success') {
                    return 'partial';
                }
                if (this.terminalType === 'run_completed') {
                    return 'completed';
                }
                if (this.isRunning) {
                    return 'running';
                }
                return this.currentRun?.runId ? 'ready' : 'idle';
            },
            panelView() {
                if (this.collapsed) {
                    return 'collapsed';
                }
                return this.detailsOpen ? 'details' : 'events';
            },
            runProjection() {
                return projectAgentInvocations(this.timelineProjection);
            },
            displayItems() {
                return timelineItemsFromEvents(this.events, {
                    foregroundInvocationIds: this.timelineProjection.foregroundInvocationIds,
                    delegationEdges: this.timelineProjection.delegationEdges,
                });
            },
            virtualDisplayItems() {
                return virtualizeTimelineItems(
                    this.displayItems,
                    this.timelineScrollTop,
                    this.timelineViewportHeight,
                );
            },
            latestDisplaySeq() {
                return this.latestDisplayItem?.seq ?? null;
            },
            selectedDisplaySeq() {
                return this.selectedItem?.seq ?? null;
            },
            activeDisplaySeq() {
                return this.isRunning ? this.latestDisplaySeq : null;
            },
            latestDisplayItem() {
                return this.displayItems[this.displayItems.length - 1] || null;
            },
            selectedItem() {
                if (this.selectedSeq != null) {
                    const selected = this.displayItems.find((item) => item.seq === this.selectedSeq);
                    if (selected) {
                        return selected;
                    }
                }
                return this.latestDisplayItem;
            },
            headerTitle() {
                if (this.isRunning) {
                    return tr('timelineRunning');
                }
                if (this.terminalType === 'run_failed') {
                    return tr('timelineFailed');
                }
                if (this.terminalType === 'run_cancelled') {
                    return tr('timelineCancelled');
                }
                if (this.terminalType === 'run_partial_success') {
                    return tr('timelinePartialSuccess');
                }
                if (this.terminalType === 'run_completed') {
                    return tr('timelineCompleted');
                }
                return tr('timelineReady');
            },
            headerSubtitle() {
                if (this.latestDisplayItem) {
                    return timelineItemTitle(this.latestDisplayItem);
                }
                return this.currentRun?.runId ? shortRunId(this.currentRun.runId) : tr('timelineIdle');
            },
            detailTitle() {
                return this.selectedItem ? timelineItemTitle(this.selectedItem) : tr('timelineDetails');
            },
            selectedDetailTargets() {
                if (!this.selectedItem) {
                    return [];
                }
                return buildEventDetailTargets(this.selectedItem, this.events);
            },
            selectedHasDetails() {
                return this.selectedDetailTargets.length > 0;
            },
            emptyTimelineText() {
                return this.isRunning ? tr('timelineThinking') : tr('timelineNoEvents');
            },
            navItems() {
                return this.displayItems.slice(-24);
            },
            subAgentTasks() {
                return this.runProjection.subAgentTasks;
            },
            subAgentTrayTitle() {
                const running = this.runProjection.runningSubAgentCount;
                const failed = this.runProjection.failedSubAgentCount;
                if (running > 0) {
                    return tr('timelineSubAgentsRunning', { count: running });
                }
                if (failed > 0) {
                    return tr('timelineSubAgentsFailed', { count: failed });
                }
                return tr('timelineSubAgentsCompleted', { count: this.runProjection.terminalSubAgentCount });
            },
            panelStyle() {
                if (this.isHistoryMode) {
                    return {};
                }
                if (this.panelHeightPx == null) {
                    return {};
                }
                return {
                    '--ttas-run-panel-user-height': `${this.panelHeightPx}px`,
                };
            },
        },
        watch: {
            selectedSeq() {
                if (this.detailsOpen) {
                    void this.loadDetails();
                }
            },
            detailsOpen(value) {
                if (value) {
                    void this.loadDetails();
                }
            },
        },
        async mounted() {
            if (this.isHistoryMode) {
                this.settings = {
                    agentModeEnabled: true,
                    runTimelineHeightPx: null,
                };
                await this.startTrackingRun(this.initialRun);
                return;
            }

            this.applySettings(await loadSettings());
            this.unsubscribeSettings = subscribeSettings((settings) => {
                this.applySettings(settings);
            });
            this.unsubscribeRunState = subscribeAgentRunState((state) => {
                void this.handleRunState(state.activeRun, state.lastEvent);
            });
            this.unsubscribeRunEvents = subscribeAgentRunEvents((event) => {
                this.receiveRunEvent(event);
            });
            await this.handleRunState(getActiveAgentRun(), null);
        },
        unmounted() {
            this.stopResize(false);
            this.$refs.subAgentDialog?.reset?.();
            if (this.timelineProjectionRefreshTimer) {
                clearTimeout(this.timelineProjectionRefreshTimer);
                this.timelineProjectionRefreshTimer = null;
            }
            this.unsubscribeSettings?.();
            this.unsubscribeRunState?.();
            this.unsubscribeRunEvents?.();
        },
        methods: {
            tr(key, params) {
                return tr(key, params);
            },
            readAgentEvents(input) {
                return requireHostApi('agent').readEvents(input);
            },
            applySettings(settings) {
                this.settings = settings;
                this.panelHeightPx = normalizeRunTimelineHeightPx(settings?.runTimelineHeightPx);
            },
            async handleRunState(activeRun, lastEvent) {
                this.activeRun = activeRun || null;
                if (activeRun?.runId && activeRun.runId !== this.currentRun?.runId) {
                    await this.startTrackingRun(activeRun);
                }
                if (lastEvent) {
                    this.receiveRunEvent(lastEvent);
                }
            },
            async startTrackingRun(run) {
                this.currentRun = run;
                this.timelineSession.reset({
                    runId: run.runId,
                    includeTimelineProjection: true,
                });
                this.selectedSeq = null;
                this.collapsed = Boolean(this.initialCollapsed);
                this.detailsOpen = false;
                this.timelineScrollTop = 0;
                this.subAgentTrayExpanded = false;
                this.detail.reset();
                this.$refs.subAgentDialog?.reset?.();
                await this.loadRunHistory();
            },
            async loadRunHistory() {
                try {
                    const applied = await this.timelineSession.loadInitial(this.readAgentEvents);
                    if (applied) {
                        this.$nextTick(() => {
                            this.measureTimelineViewport();
                            this.stickTimelineToBottom();
                        });
                    }
                } catch (error) {
                    console.error('[AgentSystem] Failed to load Agent run events', error);
                    window.toastr?.error?.(errorText(error));
                }
            },
            async loadOlderRunHistory() {
                if (!this.currentRun?.runId) {
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
                    console.error('[AgentSystem] Failed to load older Agent run events', error);
                    window.toastr?.error?.(errorText(error));
                }
            },
            receiveRunEvents(events) {
                this.timelineSession.receiveEvents(events);
                this.$refs.subAgentDialog?.receiveEvents?.(events);
                this.$nextTick(() => this.stickToBottomIfNeeded());
            },
            receiveRunEvent(event, options = {}) {
                if (!event?.runId) {
                    return;
                }
                if (!this.currentRun?.runId) {
                    this.currentRun = this.activeRun || { runId: event.runId };
                }

                const addedToRun = this.timelineSession.receiveEvent(event);
                this.$refs.subAgentDialog?.receiveEvent?.(event);
                if (!addedToRun) {
                    return;
                }
                if (!this.readOnly && event.type === 'run_failed' && event?.payload?.userRetryable === true) {
                    this.revealUserRetryableFailure(event);
                }
                if (isTimelineProjectionStructuralEvent(event.type)) {
                    this.scheduleTimelineProjectionRefresh();
                }
                if (this.detailsOpen
                    && this.selectedSeq == null
                    && (isDisplayableRunEvent(event) || hasModelTurnNarration(event))) {
                    void this.loadDetails();
                }
                if (!options.skipStick) {
                    this.$nextTick(() => this.stickToBottomIfNeeded());
                }
            },
            scheduleTimelineProjectionRefresh() {
                if (this.timelineProjectionRefreshTimer) {
                    clearTimeout(this.timelineProjectionRefreshTimer);
                }
                this.timelineProjectionRefreshTimer = setTimeout(() => {
                    this.timelineProjectionRefreshTimer = null;
                    void this.refreshTimelineProjection();
                }, 120);
            },
            async refreshTimelineProjection() {
                try {
                    await this.timelineSession.refreshProjection(this.readAgentEvents);
                } catch (error) {
                    console.error('[AgentSystem] Failed to refresh Agent timeline projection', error);
                    window.toastr?.error?.(errorText(error), tr('agentSystem'));
                }
            },
            revealUserRetryableFailure(event) {
                this.collapsed = false;
                this.selectedSeq = Number(event?.seq || 0) || this.selectedSeq;
                this.detailsOpen = true;
            },
            async invokeDetailAction(action) {
                if (!action) {
                    return;
                }
                if (action.kind === 'openSubAgent') {
                    this.openSubAgent(action.invocationId);
                    return;
                }
                if (action.kind !== 'retry') {
                    return;
                }
                if (this.readOnly) {
                    return;
                }
                try {
                    await retryAgentRunFailure({
                        run: this.currentRun,
                        events: this.events,
                        terminalEvent: this.terminalEvent,
                    });
                } catch (error) {
                    console.error('[AgentSystem] Failed to retry Agent run', error);
                    window.toastr?.error?.(errorText(error), tr('agentSystem'));
                }
            },
            selectItem(item) {
                const previousSeq = this.selectedSeq;
                this.selectedSeq = item.seq;
                if (this.detailsOpen && previousSeq === item.seq) {
                    void this.loadDetails();
                }
            },
            selectNavItem(item) {
                const previousSeq = this.selectedSeq;
                this.selectedSeq = item.seq;
                if (this.detailsOpen && previousSeq === item.seq) {
                    void this.loadDetails();
                }
            },
            toggleSubAgentTray() {
                this.subAgentTrayExpanded = !this.subAgentTrayExpanded;
            },
            selectSubAgentTask(task) {
                if (!task?.childInvocationId) {
                    return;
                }
                this.openSubAgent(task.childInvocationId);
            },
            openSubAgent(invocationId) {
                const normalized = String(invocationId || '').trim();
                if (!normalized) {
                    throw new Error('SubAgent invocationId is required.');
                }
                if (!this.currentRun?.runId) {
                    throw new Error('Agent run id is required.');
                }

                const dialog = this.$refs.subAgentDialog;
                if (!dialog || typeof dialog.open !== 'function') {
                    throw new Error(tr('subAgentDialogUnsupported'));
                }

                const task = this.subAgentTasks.find((candidate) => (
                    String(candidate?.childInvocationId || '').trim() === normalized
                )) || null;
                dialog.open({
                    runId: this.currentRun.runId,
                    invocationId: normalized,
                    task,
                    readOnly: this.readOnly,
                });
            },
            toggleCollapsed() {
                this.collapsed = !this.collapsed;
                if (!this.collapsed) {
                    this.$nextTick(() => {
                        this.measureTimelineViewport();
                        this.stickToBottomIfNeeded();
                    });
                }
            },
            openDetails() {
                if (!this.selectedHasDetails) {
                    return;
                }
                this.detailsOpen = true;
            },
            showTimeline() {
                this.detailsOpen = false;
                this.detail.reset();
                this.$nextTick(() => {
                    this.measureTimelineViewport();
                    this.stickToBottomIfNeeded();
                });
            },
            onTimelineViewport(viewport) {
                this.timelineScrollTop = viewport.scrollTop;
                this.timelineViewportHeight = viewport.viewportHeight;
                this.autoStick = viewport.nearBottom;
            },
            measureTimelineViewport() {
                if (this.collapsed || this.detailsOpen) {
                    return;
                }
                this.$refs.timelineList?.measureViewport?.();
            },
            stickTimelineToBottom() {
                this.$refs.timelineList?.scrollToBottom?.();
            },
            stickToBottomIfNeeded() {
                if (!this.autoStick || this.collapsed || this.detailsOpen) {
                    return;
                }
                this.stickTimelineToBottom();
            },
            async loadDetails() {
                const item = this.selectedItem;
                if (!item || !this.currentRun?.runId) {
                    this.detail.reset();
                    return;
                }

                await this.detail.load({
                    runId: this.currentRun.runId,
                    targets: this.selectedDetailTargets,
                    readOnly: this.readOnly,
                });
            },
            measureResizeBounds() {
                const panel = this.$refs.panelRoot;
                const body = this.$refs.panelBody;
                const header = this.$refs.panelHeader;
                if (!(panel instanceof HTMLElement) || !(body instanceof HTMLElement) || !(header instanceof HTMLElement)) {
                    throw new Error('Agent run timeline resize elements are unavailable.');
                }

                const topBar = document.getElementById('top-bar');
                const viewportTop = window.visualViewport?.offsetTop || 0;
                const topBoundary = Math.max(
                    viewportTop,
                    topBar instanceof HTMLElement ? topBar.getBoundingClientRect().bottom : 0,
                );

                return runTimelineHeightBounds({
                    panelBottom: panel.getBoundingClientRect().bottom,
                    topBoundary,
                    chromeHeight: header.getBoundingClientRect().height,
                });
            },
            currentPanelHeightPx() {
                const body = this.$refs.panelBody;
                if (!(body instanceof HTMLElement)) {
                    throw new Error('Agent run timeline body is unavailable.');
                }
                return Math.round(body.getBoundingClientRect().height);
            },
            startResize(event) {
                if (!this.canResize || this.collapsed) {
                    return;
                }

                event.preventDefault();
                this.resizeBounds = this.measureResizeBounds();
                this.resizeStartY = event.clientY;
                this.resizeStartHeightPx = clampRunTimelineHeightPx(
                    this.panelHeightPx ?? this.currentPanelHeightPx(),
                    this.resizeBounds,
                );
                this.panelHeightPx = this.resizeStartHeightPx;
                this.resizing = true;
                event.currentTarget.setPointerCapture(event.pointerId);

                window.addEventListener('pointermove', this.onResizePointerMove);
                window.addEventListener('pointerup', this.onResizePointerUp);
                window.addEventListener('pointercancel', this.onResizePointerCancel);
            },
            onResizePointerMove(event) {
                if (!this.resizing) {
                    return;
                }
                this.panelHeightPx = heightFromTopEdgeDrag({
                    startHeight: this.resizeStartHeightPx,
                    startY: this.resizeStartY,
                    currentY: event.clientY,
                    bounds: this.resizeBounds,
                });
            },
            onResizePointerUp() {
                void this.stopResize(true);
            },
            onResizePointerCancel() {
                void this.stopResize(false);
            },
            async stopResize(save) {
                window.removeEventListener('pointermove', this.onResizePointerMove);
                window.removeEventListener('pointerup', this.onResizePointerUp);
                window.removeEventListener('pointercancel', this.onResizePointerCancel);

                if (!this.resizing) {
                    return;
                }

                this.resizing = false;
                if (save) {
                    await this.savePanelHeight(this.panelHeightPx);
                }
            },
            async savePanelHeight(heightPx) {
                if (!this.canResize) {
                    return;
                }
                this.applySettings(await patchSettings(this.settings, {
                    runTimelineHeightPx: normalizeRunTimelineHeightPx(heightPx),
                }));
            },
            async resetPanelHeight() {
                if (!this.canResize) {
                    return;
                }
                this.applySettings(await patchSettings(this.settings, {
                    runTimelineHeightPx: null,
                }));
            },
            async onResizeKeydown(event) {
                if (!this.canResize) {
                    return;
                }
                const bounds = this.measureResizeBounds();
                const current = clampRunTimelineHeightPx(
                    this.panelHeightPx ?? this.currentPanelHeightPx(),
                    bounds,
                );
                let next = null;

                if (event.key === 'ArrowUp') {
                    next = current + RUN_TIMELINE_KEYBOARD_STEP_PX;
                } else if (event.key === 'ArrowDown') {
                    next = current - RUN_TIMELINE_KEYBOARD_STEP_PX;
                } else if (event.key === 'PageUp') {
                    next = current + RUN_TIMELINE_PAGE_STEP_PX;
                } else if (event.key === 'PageDown') {
                    next = current - RUN_TIMELINE_PAGE_STEP_PX;
                } else if (event.key === 'Home') {
                    next = bounds.min;
                } else if (event.key === 'End') {
                    next = bounds.max;
                }

                if (next == null) {
                    return;
                }

                event.preventDefault();
                this.panelHeightPx = clampRunTimelineHeightPx(next, bounds);
                await this.savePanelHeight(this.panelHeightPx);
            },
            closeTimeline() {
                if (typeof this.requestClose === 'function') {
                    this.requestClose();
                }
            },
        },
        template: `
            <section
                ref="panelRoot"
                v-show="visible"
                :id="rootId"
                class="ttas-root ttas-run-panel"
                :class="{
                    'is-collapsed': collapsed,
                    'is-history': isHistoryMode,
                    'is-running': isRunning,
                    'is-details-open': detailsOpen,
                    'is-terminal': terminalType,
                    'is-error': terminalType === 'run_failed',
                    'is-warning': terminalType === 'run_partial_success',
                    'is-resizing': resizing,
                }"
                :data-ttas-status="panelStatus"
                :data-ttas-view="panelView"
                :style="panelStyle"
                aria-live="polite"
            >
                <button
                    v-if="canResize && !collapsed"
                    type="button"
                    class="ttas-run-resize-handle"
                    :title="tr('resizeTimelineHeight')"
                    :aria-label="tr('resizeTimelineHeight')"
                    role="separator"
                    aria-orientation="horizontal"
                    @pointerdown="startResize"
                    @dblclick="resetPanelHeight"
                    @keydown="onResizeKeydown"
                ></button>
                <header ref="panelHeader" class="ttas-run-header">
                    <div class="ttas-run-heading">
                        <span class="ttas-run-orb" aria-hidden="true">
                            <i class="fa-solid fa-wand-magic-sparkles"></i>
                        </span>
                        <div class="ttas-run-heading-copy">
                            <strong>{{ headerTitle }}</strong>
                            <small>{{ headerSubtitle }}</small>
                        </div>
                    </div>
                    <div class="ttas-run-actions">
                        <button
                            type="button"
                            class="menu_button menu_button_icon ttas-run-icon-button"
                            :title="detailsOpen ? tr('showTimelineEvents') : tr('showTimelineDetails')"
                            :aria-label="detailsOpen ? tr('showTimelineEvents') : tr('showTimelineDetails')"
                            :disabled="collapsed || (!detailsOpen && (!selectedItem || !selectedHasDetails))"
                            @click="detailsOpen ? showTimeline() : openDetails()"
                        >
                            <i class="fa-solid" :class="detailsOpen ? 'fa-list' : 'fa-circle-info'"></i>
                        </button>
                        <button
                            v-if="isHistoryMode"
                            type="button"
                            class="menu_button menu_button_icon ttas-run-icon-button"
                            :title="tr('close')"
                            :aria-label="tr('close')"
                            @click="closeTimeline"
                        >
                            <i class="fa-solid fa-xmark"></i>
                        </button>
                        <button
                            v-else
                            type="button"
                            class="menu_button menu_button_icon ttas-run-icon-button"
                            :title="collapsed ? tr('expandTimeline') : tr('collapseTimeline')"
                            :aria-label="collapsed ? tr('expandTimeline') : tr('collapseTimeline')"
                            :aria-expanded="String(!collapsed)"
                            @click="toggleCollapsed"
                        >
                            <i class="fa-solid" :class="collapsed ? 'fa-chevron-up' : 'fa-chevron-down'"></i>
                        </button>
                    </div>
                </header>

                <div v-if="!collapsed" ref="panelBody" class="ttas-run-body">
                    <section
                        v-show="!detailsOpen"
                        class="ttas-run-view ttas-run-view-events"
                        :aria-label="tr('agentTimeline')"
                    >
                        <RunTimelineEventList
                            ref="timelineList"
                            :aria-label="tr('agentTimeline')"
                            surface-class="ttas-run-event-scroll"
                            :loading="loadingHistory"
                            :loading-older="loadingOlderHistory"
                            :empty-text="emptyTimelineText"
                            :items="displayItems"
                            :virtual-items="virtualDisplayItems"
                            :selected-seq="selectedDisplaySeq"
                            :latest-seq="latestDisplaySeq"
                            :active-seq="activeDisplaySeq"
                            @select="selectItem"
                            @top-reached="loadOlderRunHistory"
                            @viewport="onTimelineViewport"
                        />
                        <SubAgentTray
                            :expanded="subAgentTrayExpanded"
                            :tasks="subAgentTasks"
                            :title="subAgentTrayTitle"
                            @toggle="toggleSubAgentTray"
                            @select="selectSubAgentTask"
                        />
                    </section>

                    <RunTimelineDetailPane
                        v-if="detailsOpen"
                        root-class="ttas-run-view ttas-run-view-details"
                        :aria-label="tr('timelineDetails')"
                        :title="detailTitle"
                        :type="selectedItem ? selectedItem.type : ''"
                        :nav-items="navItems"
                        :selected-seq="selectedDisplaySeq"
                        :loading="detail.loading"
                        :error="detail.error"
                        :sections="detail.sections"
                        :show-back="true"
                        @back="showTimeline"
                        @select-nav="selectNavItem"
                        @action="invokeDetailAction"
                    />
                </div>
                <SubAgentTimelineDialog
                    ref="subAgentDialog"
                    @action="invokeDetailAction"
                />
            </section>
        `,
    });
}

function normalizeTimelineOptions(options) {
    const mode = options?.mode === 'history' ? 'history' : 'active';
    if (mode === 'history') {
        const runId = String(options?.run?.runId || '').trim();
        if (!runId) {
            throw new Error('Agent run id is required.');
        }
        return {
            mode,
            rootId: `ttas_agent_run_timeline_history_${++historyTimelineDialogCounter}`,
            readOnly: true,
            requestClose: typeof options.requestClose === 'function' ? options.requestClose : null,
            run: {
                ...options.run,
                runId,
            },
            collapsed: false,
        };
    }

    return {
        mode: 'active',
        rootId: 'ttas_agent_run_timeline',
        readOnly: false,
        requestClose: null,
        run: null,
        collapsed: true,
    };
}

export function openAgentRunTimelineDialog(run) {
    const runId = String(run?.runId || '').trim();
    if (!runId) {
        throw new Error('Agent run id is required.');
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('runHistoryDialogUnsupported'));
    }

    const dialog = document.createElement('dialog');
    dialog.className = 'ttas-dialog ttas-run-history-dialog';
    dialog.dataset.ttMobileSurface = 'fullscreen-window';

    const mount = document.createElement('div');
    mount.className = 'ttas-run-history-dialog-mount';
    dialog.append(mount);
    document.body.append(dialog);
    if (typeof dialog.showModal !== 'function') {
        dialog.remove();
        throw new Error(tr('runHistoryDialogUnsupported'));
    }

    let app = null;
    const close = () => {
        if (dialog.open) {
            dialog.close();
        } else {
            dialog.remove();
        }
    };

    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        close();
    });
    dialog.addEventListener('close', () => {
        app?.unmount();
        dialog.remove();
    }, { once: true });

    app = createAgentRunTimelineApp({
        mode: 'history',
        run: { ...run, runId },
        requestClose: close,
    });
    app.mount(mount);
    dialog.showModal();
}

export async function mountAgentRunTimelinePanel() {
    const sendForm = document.getElementById('send_form');
    if (!(sendForm instanceof HTMLElement) || !(sendForm.parentElement instanceof HTMLElement)) {
        throw new Error(tr('sendFormNotFound'));
    }

    if (document.getElementById(MOUNT_ID)) {
        return;
    }

    const mount = document.createElement('div');
    mount.id = MOUNT_ID;
    mount.className = 'ttas-run-timeline-mount';
    sendForm.parentElement.insertBefore(mount, sendForm);
    createAgentRunTimelineApp().mount(mount);
}
