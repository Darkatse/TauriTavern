import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { errorText, requireHostApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import { formatDetailFile, formatModelTurnDetail, formatPatchDiffDetail } from './run-detail-format.js';
import {
    clampRunTimelineHeightPx,
    heightFromTopEdgeDrag,
    normalizeRunTimelineHeightPx,
    RUN_TIMELINE_KEYBOARD_STEP_PX,
    RUN_TIMELINE_PAGE_STEP_PX,
    runTimelineHeightBounds,
} from './run-timeline-resize.js';
import {
    getActiveAgentRun,
    subscribeAgentRunEvents,
    subscribeAgentRunState,
} from '../../../tauritavern/agent/agent-run-controller.js';
import {
    buildEventDetailTargets,
    isDisplayableRunEvent,
    timelineItemsFromEvents,
    TERMINAL_EVENT_TYPES,
} from './run-event-presenter.js';

const MOUNT_ID = 'ttas_agent_run_timeline_mount';
const HISTORY_LIMIT = 160;
const MAX_RAW_EVENTS = 260;

function createAgentRunTimelineApp() {
    return createApp({
        data() {
            return {
                settings: {
                    agentModeEnabled: false,
                    runTimelineHeightPx: null,
                },
                currentRun: null,
                activeRun: null,
                events: [],
                seenEventKeys: Object.create(null),
                terminalEvent: null,
                collapsed: true,
                detailsOpen: false,
                selectedSeq: null,
                autoStick: true,
                loadingHistory: false,
                historyRequestId: 0,
                detailLoading: false,
                detailError: '',
                detailSections: [],
                detailRequestId: 0,
                panelHeightPx: null,
                resizing: false,
                resizeStartY: 0,
                resizeStartHeightPx: 0,
                resizeBounds: null,
                unsubscribeSettings: null,
                unsubscribeRunState: null,
                unsubscribeRunEvents: null,
            };
        },
        computed: {
            visible() {
                return Boolean(this.settings.agentModeEnabled);
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
            displayItems() {
                return timelineItemsFromEvents(this.events).slice(-90);
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
                if (this.terminalType === 'run_completed') {
                    return tr('timelineCompleted');
                }
                return tr('timelineReady');
            },
            headerSubtitle() {
                if (this.latestDisplayItem) {
                    return this.itemTitle(this.latestDisplayItem);
                }
                return this.currentRun?.runId ? this.shortRunId(this.currentRun.runId) : tr('timelineIdle');
            },
            detailTitle() {
                return this.selectedItem ? this.itemTitle(this.selectedItem) : tr('timelineDetails');
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
            panelStyle() {
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
            this.unsubscribeSettings?.();
            this.unsubscribeRunState?.();
            this.unsubscribeRunEvents?.();
        },
        methods: {
            tr(key, params) {
                return tr(key, params);
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
                this.events = [];
                this.seenEventKeys = Object.create(null);
                this.terminalEvent = null;
                this.selectedSeq = null;
                this.collapsed = true;
                this.detailsOpen = false;
                this.detailSections = [];
                this.detailError = '';
                await this.loadRunHistory(run.runId);
            },
            async loadRunHistory(runId) {
                const requestId = ++this.historyRequestId;
                this.loadingHistory = true;
                try {
                    const result = await requireHostApi('agent').readEvents({
                        runId,
                        afterSeq: 0,
                        limit: HISTORY_LIMIT,
                    });
                    if (requestId !== this.historyRequestId) {
                        return;
                    }
                    for (const event of Array.isArray(result?.events) ? result.events : []) {
                        this.receiveRunEvent(event);
                    }
                } catch (error) {
                    console.error('[AgentSystem] Failed to load Agent run events', error);
                    window.toastr?.error?.(errorText(error));
                } finally {
                    if (requestId === this.historyRequestId) {
                        this.loadingHistory = false;
                    }
                }
            },
            receiveRunEvent(event) {
                if (!event?.runId) {
                    return;
                }
                if (!this.currentRun?.runId) {
                    this.currentRun = this.activeRun || { runId: event.runId };
                }
                if (event.runId !== this.currentRun.runId) {
                    return;
                }

                const key = this.eventKey(event);
                if (this.seenEventKeys[key]) {
                    return;
                }
                this.seenEventKeys[key] = true;
                this.events.push(event);
                this.events.sort((a, b) => Number(a?.seq || 0) - Number(b?.seq || 0));
                if (this.events.length > MAX_RAW_EVENTS) {
                    this.events.splice(0, this.events.length - MAX_RAW_EVENTS);
                }
                if (TERMINAL_EVENT_TYPES.includes(event.type)) {
                    this.terminalEvent = event;
                }
                if (this.detailsOpen && this.selectedSeq == null && isDisplayableRunEvent(event)) {
                    void this.loadDetails();
                }
                this.$nextTick(() => this.stickToBottomIfNeeded());
            },
            eventKey(event) {
                return event?.id ? `id:${event.id}` : `seq:${event?.runId || ''}:${event?.seq || 0}`;
            },
            itemTitle(item) {
                return tr(item.titleKey, item.titleParams || {});
            },
            itemShortLabel(item) {
                switch (String(item?.kind || '')) {
                    case 'read':
                        return tr('timelineOpRead');
                    case 'search':
                        return tr('timelineOpSearch');
                    case 'list':
                        return tr('timelineOpList');
                    case 'write':
                        return tr('timelineOpWrite');
                    case 'patch':
                        return tr('timelineOpPatch');
                    case 'commit':
                        return tr('timelineOpCommit');
                    case 'persist':
                        return tr('timelineOpPersist');
                    case 'done':
                        return tr('timelineOpDone');
                    case 'fail':
                        return tr('timelineOpFail');
                    case 'cancel':
                        return tr('timelineOpCancel');
                    default:
                        break;
                }

                const type = String(item?.type || '');
                if (type === 'workspace_file_written') {
                    return tr('timelineOpWrite');
                }
                if (type === 'workspace_patch_applied') {
                    return tr('timelineOpPatch');
                }
                if (type === 'chat_commit_completed' || type === 'chat_commit_requested') {
                    return tr('timelineOpCommit');
                }
                if (type === 'persistent_changes_committed') {
                    return tr('timelineOpPersist');
                }
                if (type === 'run_completed') {
                    return tr('timelineOpDone');
                }
                if (type === 'run_failed' || type === 'tool_call_failed' || type === 'chat_commit_failed') {
                    return tr('timelineOpFail');
                }
                if (type === 'run_cancelled') {
                    return tr('timelineOpCancel');
                }

                const tool = String(item?.rawEvent?.payload?.name || item?.titleParams?.tool || '');
                if (tool.includes('read')) {
                    return tr('timelineOpRead');
                }
                if (tool.includes('search')) {
                    return tr('timelineOpSearch');
                }
                if (tool.includes('list')) {
                    return tr('timelineOpList');
                }
                return tr('timelineOpTool');
            },
            itemTime(item) {
                if (!item.timestamp) {
                    return '';
                }
                const date = new Date(item.timestamp);
                if (Number.isNaN(date.getTime())) {
                    return '';
                }
                return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
            },
            shortRunId(runId) {
                const value = String(runId || '');
                if (value.length <= 14) {
                    return value;
                }
                return `${value.slice(0, 10)}...`;
            },
            isLatestActiveItem(item) {
                return Boolean(this.isRunning && this.latestDisplayItem?.seq === item.seq);
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
            toggleCollapsed() {
                this.collapsed = !this.collapsed;
            },
            openDetails() {
                if (!this.selectedHasDetails) {
                    return;
                }
                this.detailsOpen = true;
                this.$nextTick(() => {
                    const pages = this.$refs.pages;
                    if (pages instanceof HTMLElement) {
                        pages.scrollTo({ left: pages.clientWidth, behavior: 'smooth' });
                    }
                });
            },
            showTimeline() {
                this.detailsOpen = false;
                this.$nextTick(() => {
                    const pages = this.$refs.pages;
                    if (pages instanceof HTMLElement) {
                        pages.scrollTo({ left: 0, behavior: 'smooth' });
                    }
                });
            },
            onPagesScroll() {
                const pages = this.$refs.pages;
                if (!(pages instanceof HTMLElement)) {
                    return;
                }
                this.detailsOpen = pages.scrollLeft > pages.clientWidth * 0.55;
            },
            onTimelineScroll() {
                const scroller = this.$refs.timelineScroller;
                if (!(scroller instanceof HTMLElement)) {
                    return;
                }
                const remaining = scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop;
                this.autoStick = remaining < 18;
            },
            stickToBottomIfNeeded() {
                if (!this.autoStick || this.collapsed) {
                    return;
                }
                const scroller = this.$refs.timelineScroller;
                if (scroller instanceof HTMLElement) {
                    scroller.scrollTop = scroller.scrollHeight;
                }
            },
            async loadDetails() {
                const item = this.selectedItem;
                if (!item || !this.currentRun?.runId) {
                    this.detailSections = [];
                    this.detailError = '';
                    return;
                }

                const requestId = ++this.detailRequestId;
                this.detailLoading = true;
                this.detailError = '';
                try {
                    const targets = this.selectedDetailTargets;
                    const sections = [];
                    for (const target of targets) {
                        sections.push(await this.readDetailFile(target));
                    }
                    if (requestId === this.detailRequestId) {
                        this.detailSections = sections;
                    }
                } catch (error) {
                    if (requestId === this.detailRequestId) {
                        this.detailError = errorText(error);
                        this.detailSections = [];
                    }
                } finally {
                    if (requestId === this.detailRequestId) {
                        this.detailLoading = false;
                    }
                }
            },
            async readDetailFile(target) {
                if (target.type === 'modelTurn' || target.type === 'modelReasoning') {
                    const turn = await requireHostApi('agent').readModelTurn({
                        runId: this.currentRun.runId,
                        round: target.round,
                    });
                    return formatModelTurnDetail(target, turn);
                }
                if (target.type === 'patchDiff') {
                    if (target.errorKey) {
                        throw new Error(tr(target.errorKey, target.errorParams || {}));
                    }
                    const file = await requireHostApi('agent').readWorkspaceFile({
                        runId: this.currentRun.runId,
                        path: target.argumentsRef,
                    });
                    return formatPatchDiffDetail(target, file);
                }
                const file = await requireHostApi('agent').readWorkspaceFile({
                    runId: this.currentRun.runId,
                    path: target.path,
                });
                return formatDetailFile(target, file);
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
                if (this.collapsed) {
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
                this.applySettings(await patchSettings(this.settings, {
                    runTimelineHeightPx: normalizeRunTimelineHeightPx(heightPx),
                }));
            },
            async resetPanelHeight() {
                this.applySettings(await patchSettings(this.settings, {
                    runTimelineHeightPx: null,
                }));
            },
            async onResizeKeydown(event) {
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
        },
        template: `
            <section
                ref="panelRoot"
                v-show="visible"
                id="ttas_agent_run_timeline"
                class="ttas-root ttas-run-panel"
                :class="{
                    'is-collapsed': collapsed,
                    'is-running': isRunning,
                    'is-details-open': detailsOpen,
                    'is-terminal': terminalType,
                    'is-error': terminalType === 'run_failed',
                    'is-resizing': resizing,
                }"
                :data-ttas-status="panelStatus"
                :data-ttas-view="panelView"
                :style="panelStyle"
                aria-live="polite"
            >
                <button
                    v-if="!collapsed"
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
                    <div ref="pages" class="ttas-run-pages" @scroll.passive="onPagesScroll">
                        <section class="ttas-run-page ttas-run-page-events" :aria-label="tr('agentTimeline')">
                            <div ref="timelineScroller" class="ttas-run-event-scroll" @scroll.passive="onTimelineScroll">
                                <div v-if="loadingHistory && displayItems.length === 0" class="ttas-run-empty">
                                    <i class="fa-solid fa-spinner fa-spin"></i>
                                    <span>{{ tr('timelineLoading') }}</span>
                                </div>
                                <div v-else-if="displayItems.length === 0" class="ttas-run-empty">
                                    <i class="fa-solid fa-circle-dot"></i>
                                    <span>{{ emptyTimelineText }}</span>
                                </div>
                                <ol v-else class="ttas-run-events">
                                    <li
                                        v-for="item in displayItems"
                                        :key="item.id"
                                        class="ttas-run-event"
                                        :data-ttas-kind="item.kind"
                                        :class="[
                                            'tone-' + item.tone,
                                            'kind-' + item.kind,
                                            {
                                                'is-latest': latestDisplayItem && latestDisplayItem.seq === item.seq,
                                                'is-active': isLatestActiveItem(item),
                                                'is-selected': selectedItem && selectedItem.seq === item.seq,
                                            },
                                        ]"
                                    >
                                        <button type="button" @click="selectItem(item)">
                                            <span class="ttas-run-event-icon" aria-hidden="true">
                                                <i class="fa-solid" :class="item.icon"></i>
                                            </span>
                                            <span class="ttas-run-event-copy">
                                                <span class="ttas-run-event-title">
                                                    {{ itemTitle(item) }}
                                                    <span v-if="isLatestActiveItem(item)" class="ttas-run-ellipsis" aria-hidden="true">
                                                        <i>.</i><i>.</i><i>.</i>
                                                    </span>
                                                </span>
                                                <small v-if="item.summary">{{ item.summary }}</small>
                                            </span>
                                            <span class="ttas-run-event-meta">
                                                <em>{{ itemShortLabel(item) }}</em>
                                                <time v-if="itemTime(item)">{{ itemTime(item) }}</time>
                                            </span>
                                        </button>
                                    </li>
                                </ol>
                            </div>
                        </section>

                        <section class="ttas-run-page ttas-run-page-details" :aria-label="tr('timelineDetails')">
                            <div class="ttas-run-detail-head">
                                <button
                                    type="button"
                                    class="menu_button menu_button_icon ttas-run-icon-button"
                                    :title="tr('showTimelineEvents')"
                                    :aria-label="tr('showTimelineEvents')"
                                    @click="showTimeline"
                                >
                                    <i class="fa-solid fa-arrow-left"></i>
                                </button>
                                <div>
                                    <strong>{{ detailTitle }}</strong>
                                    <small v-if="selectedItem">{{ selectedItem.type }}</small>
                                </div>
                            </div>

                            <div
                                v-if="navItems.length > 1"
                                class="ttas-run-detail-nav"
                            >
                                <div class="ttas-run-nav-list">
                                    <button
                                        v-for="item in navItems"
                                        :key="'nav-' + item.id"
                                        type="button"
                                        :class="{ 'is-selected': selectedItem && selectedItem.seq === item.seq }"
                                        :title="itemTitle(item)"
                                        @click.stop="selectNavItem(item)"
                                    >
                                        <i aria-hidden="true"></i>
                                        <span>{{ itemShortLabel(item) }}</span>
                                    </button>
                                </div>
                            </div>

                            <div class="ttas-run-detail-scroll">
                                <div v-if="detailLoading" class="ttas-run-empty">
                                    <i class="fa-solid fa-spinner fa-spin"></i>
                                    <span>{{ tr('timelineLoadingDetails') }}</span>
                                </div>
                                <div v-else-if="detailError" class="ttas-run-detail-error">
                                    <i class="fa-solid fa-triangle-exclamation"></i>
                                    <span>{{ detailError }}</span>
                                </div>
                                <div v-else-if="detailSections.length === 0" class="ttas-run-empty">
                                    <i class="fa-solid fa-file-circle-question"></i>
                                    <span>{{ tr('timelineDetailEmpty') }}</span>
                                </div>
                                <article v-for="(section, index) in detailSections" :key="index" class="ttas-run-detail-section">
                                    <div class="ttas-run-detail-section-head">
                                        <strong>{{ tr(section.labelKey) }}</strong>
                                        <small v-if="section.path">{{ section.path }}</small>
                                    </div>
                                    <div v-if="section.fields && section.fields.length" class="ttas-run-detail-fields">
                                        <span v-for="field in section.fields" :key="field.label">
                                            <b>{{ field.label }}</b>
                                            <em>{{ field.value }}</em>
                                        </span>
                                    </div>
                                    <div v-if="section.blocks && section.blocks.length" class="ttas-run-detail-blocks">
                                        <details
                                            v-for="(block, blockIndex) in section.blocks"
                                            :key="blockIndex"
                                            class="ttas-run-detail-block"
                                            :class="'kind-' + (block.kind || 'text')"
                                            :data-ttas-block-kind="block.kind || 'text'"
                                            :open="block.defaultOpen !== false"
                                        >
                                            <summary class="ttas-run-detail-block-head">
                                                <strong>{{ block.labelKey ? tr(block.labelKey) : block.label }}</strong>
                                                <span class="ttas-run-detail-block-badges">
                                                    <small v-if="block.meta">{{ block.meta }}</small>
                                                    <small v-if="block.truncated">{{ tr('timelineTruncated') }}</small>
                                                    <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                                                </span>
                                            </summary>
                                            <div v-if="block.kind === 'diff'" class="ttas-run-diff" role="table">
                                                <div
                                                    v-for="(row, rowIndex) in block.rows"
                                                    :key="rowIndex"
                                                    class="ttas-run-diff-row"
                                                    :data-ttas-diff-row="row.type"
                                                    role="row"
                                                >
                                                    <span class="ttas-run-diff-gutter" role="cell">{{ row.oldLine || '' }}</span>
                                                    <span class="ttas-run-diff-gutter" role="cell">{{ row.newLine || '' }}</span>
                                                    <span class="ttas-run-diff-marker" role="cell">{{ row.marker }}</span>
                                                    <code class="ttas-run-diff-code" role="cell">{{ row.text }}</code>
                                                </div>
                                            </div>
                                            <pre v-else>{{ block.text }}</pre>
                                        </details>
                                    </div>
                                </article>
                            </div>
                        </section>
                    </div>
                </div>
            </section>
        `,
    });
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
