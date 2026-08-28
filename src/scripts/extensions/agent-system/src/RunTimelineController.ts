import type { AgentSystemSettings } from './settings-store';
import type {
    RunTimelineController,
    RunTimelineOptions,
    RunTimelineSnapshot,
    SubAgentTask,
    TimelineDetailTarget,
    TimelineItem,
    TimelineResizeBounds,
    TimelineRun,
    TimelineViewport,
} from './RunTimelineContract';
import {
    buildEventDetailTargets,
    hasModelTurnNarration,
    isDisplayableRunEvent,
    timelineItemsFromEvents,
} from './run-event-presenter';
import { projectSubAgentTasks } from './run-invocation-projector';
import { createTimelineDetailState } from './run-timeline-detail-state';
import {
    runTimelineHeading,
    shortRunId,
    subAgentTrayTitle,
    timelineItemTitle,
} from './run-timeline-display';
import { isTimelineProjectionStructuralEvent } from './run-timeline-projection';
import {
    clampRunTimelineHeightPx,
    heightFromTopEdgeDrag,
    normalizeRunTimelineHeightPx,
    RUN_TIMELINE_KEYBOARD_STEP_PX,
    RUN_TIMELINE_PAGE_STEP_PX,
} from './run-timeline-resize';
import { createRunTimelineSession } from './run-timeline-session';
import { createRunTimelineLiveLane } from './run-timeline-live-lane';
import { createSubAgentTimelineController } from './SubAgentTimelineController';
import {
    canStartRunTimelineViewGesture,
    createRunTimelineViewGesture,
    resolveRunTimelineViewGesture,
    RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS,
    RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE,
    shouldCancelRunTimelineViewGesture,
    type RunTimelineViewGesture,
} from './run-timeline-view-gesture';
import { virtualizeTimelineItems } from './run-timeline-virtual-list';

const ACTIVE_ROOT_ID = 'ttas_agent_run_timeline';

type DerivedTimeline = {
    subAgentTasks: SubAgentTask[];
    items: TimelineItem[];
    navItems: TimelineItem[];
};

export function createRunTimelineController(options: RunTimelineOptions): RunTimelineController {
    const { deps } = options;
    const main = createRunTimelineSession({ includeTimelineProjection: true });
    const detail = createTimelineDetailState();
    const listeners = new Set<() => void>();
    const unsubscribes: Array<() => void> = [];
    let settings: AgentSystemSettings | null = null;
    let currentRun: TimelineRun | null = options.mode === 'history' ? options.run : null;
    let activeRun: TimelineRun | null = null;
    let collapsed = options.mode === 'active';
    let detailsOpen = false;
    let selectedSeq: number | null = null;
    let viewport: TimelineViewport = { scrollTop: 0, viewportHeight: 1, nearBottom: true };
    let trayExpanded = false;
    let panelHeightPx: number | null = null;
    let resizing = false;
    let resizeStartY = 0;
    let resizeStartHeight = 0;
    let resizeBounds: TimelineResizeBounds = { min: 0, max: 0 };
    let viewGesture: RunTimelineViewGesture | null = null;
    let projectionTimer: ReturnType<typeof setTimeout> | null = null;
    let disposed = false;
    let initPromise: Promise<void> | null = null;
    const liveDeps = options.mode === 'active' ? options.deps : null;
    const liveLane = createRunTimelineLiveLane({
        subscribeLiveProjection: liveDeps?.subscribeLiveProjection,
        scheduleFrame: liveDeps?.scheduleFrame,
        onChange: publish,
        onError: deps.reportError,
    });
    // Session arrays are immutable references; keep presentation stable across viewport-only publishes.
    let derivedEventRef: readonly TauriTavernAgentRunEvent[] = main.events;
    let derivedProjectionRef = main.timelineProjection;
    let derivedEventBase: { subAgentTasks: SubAgentTask[]; eventItems: TimelineItem[] } | null = null;
    let derivedLiveVersion = liveLane.version();
    let derived: DerivedTimeline = deriveMain();
    let selectionItemsRef: readonly TimelineItem[] | null = null;
    let selectionEventsRef: readonly TauriTavernAgentRunEvent[] | null = null;
    let selectionSeqRef: number | null = null;
    let selectionCache = { item: null as TimelineItem | null, targets: [] as TimelineDetailTarget[] };
    const subAgent = createSubAgentTimelineController({
        readEvents: deps.readEvents,
        readOnly: options.mode === 'history',
        reportError: deps.reportError,
        tr: deps.tr,
        onChange: publish,
    });
    let snapshot = buildSnapshot();

    function deriveMain(): DerivedTimeline {
        if (!derivedEventBase || derivedEventRef !== main.events || derivedProjectionRef !== main.timelineProjection) {
            derivedEventRef = main.events;
            derivedProjectionRef = main.timelineProjection;
            derivedEventBase = {
                subAgentTasks: projectSubAgentTasks(main.timelineProjection),
                eventItems: timelineItemsFromEvents(main.events, {
                    foregroundInvocationIds: main.timelineProjection.foregroundInvocationIds,
                    delegationEdges: main.timelineProjection.delegationEdges,
                }),
            };
        }
        const items = [...derivedEventBase.eventItems, ...liveLane.items()];
        return { subAgentTasks: derivedEventBase.subAgentTasks, items, navItems: items.slice(-24) };
    }

    function currentDerived(): DerivedTimeline {
        const liveVersion = liveLane.version();
        if (!derivedEventBase || derivedLiveVersion !== liveVersion
            || derivedEventRef !== main.events || derivedProjectionRef !== main.timelineProjection) {
            derivedLiveVersion = liveVersion;
            derived = deriveMain();
        }
        return derived;
    }

    function currentSelection(view: DerivedTimeline) {
        if (selectionItemsRef !== view.items || selectionEventsRef !== main.events || selectionSeqRef !== selectedSeq) {
            selectionItemsRef = view.items;
            selectionEventsRef = main.events;
            selectionSeqRef = selectedSeq;
            const item = (selectedSeq == null ? null : view.items.find(value => value.seq === selectedSeq))
                ?? view.items.at(-1) ?? null;
            selectionCache = { item, targets: item ? buildEventDetailTargets(item, main.events) : [] };
        }
        return selectionCache;
    }

    function buildSnapshot(): RunTimelineSnapshot {
        const view = currentDerived();
        const selection = currentSelection(view);
        const selectedItem = selection.item;
        const latest = view.items.at(-1) ?? null;
        const terminalType = main.terminalEvent?.type ?? '';
        const isRunning = Boolean(activeRun?.runId && currentRun?.runId === activeRun.runId);
        const heading = runTimelineHeading(terminalType, isRunning, Boolean(currentRun?.runId), deps.tr);
        const subTask = view.subAgentTasks.find(task => task.targetInvocationId === subAgent.invocationId()) ?? null;
        return {
            mode: options.mode,
            rootId: options.mode === 'history' ? options.rootId : ACTIVE_ROOT_ID,
            visible: options.mode === 'history' || settings?.agentModeEnabled === true,
            displayItems: view.items,
            virtualItems: virtualizeTimelineItems(view.items, viewport.scrollTop, viewport.viewportHeight),
            selectedItem,
            selectedSeq: selectedItem?.seq ?? null,
            latestSeq: latest?.seq ?? null,
            activeSeq: isRunning ? latest?.seq ?? null : null,
            navItems: view.navItems,
            loading: main.loading,
            loadingOlder: main.loadingOlder,
            detail: { loading: detail.loading, error: detail.error, sections: detail.sections },
            collapsed,
            detailsOpen,
            autoStick: viewport.nearBottom,
            trayExpanded,
            panelHeightPx,
            resizing,
            isRunning,
            terminalType,
            panelStatus: heading.status,
            panelView: collapsed ? 'collapsed' : detailsOpen ? 'details' : 'events',
            headerTitle: heading.title,
            headerSubtitle: latest ? timelineItemTitle(latest, deps.tr)
                : currentRun?.runId ? shortRunId(currentRun.runId) : deps.tr('timelineIdle'),
            detailTitle: selectedItem ? timelineItemTitle(selectedItem, deps.tr) : deps.tr('timelineDetails'),
            selectedHasDetails: selection.targets.length > 0,
            emptyText: isRunning ? deps.tr('timelineThinking') : deps.tr('timelineNoEvents'),
            subAgentTasks: view.subAgentTasks,
            subAgentTrayTitle: subAgentTrayTitle(view.subAgentTasks, deps.tr),
            subAgent: subAgent.snapshot(subTask),
        };
    }

    function publish(): void {
        if (disposed) return;
        snapshot = buildSnapshot();
        listeners.forEach(listener => listener());
    }

    function fire(task: Promise<unknown>): void {
        void task.catch(error => {
            deps.reportError(error);
            queueMicrotask(() => { throw error; });
        });
    }

    async function loadInitial(): Promise<boolean> {
        try {
            const pending = main.loadInitial(deps.readEvents);
            publish();
            const applied = await pending;
            publish();
            return applied;
        } catch (error) {
            publish();
            deps.reportError(error);
            return false;
        }
    }

    async function startTrackingRun(run: TimelineRun): Promise<void> {
        currentRun = run;
        main.reset({ runId: run.runId, includeTimelineProjection: true });
        selectedSeq = null;
        collapsed = options.mode === 'active';
        detailsOpen = false;
        viewport = { scrollTop: 0, viewportHeight: 1, nearBottom: true };
        trayExpanded = false;
        viewGesture = null;
        detail.reset();
        subAgent.reset();
        liveLane.attach(run.runId);
        await loadInitial();
    }

    async function handleRunState(run: TimelineRun | null, lastEvent: TauriTavernAgentRunEvent | null): Promise<void> {
        activeRun = run;
        if (run?.runId && run.runId !== currentRun?.runId) await startTrackingRun(run);
        else publish();
        if (lastEvent) receiveRunEvent(lastEvent);
    }

    function eventShowsDetails(event: TauriTavernAgentRunEvent): boolean {
        return isDisplayableRunEvent(event) || hasModelTurnNarration(event);
    }

    function receiveRunEvent(event: TauriTavernAgentRunEvent): void {
        if (!event.runId) throw new Error('Agent run event runId is required.');
        if (!currentRun) currentRun = activeRun ?? { runId: event.runId };
        const added = main.receiveEvent(event);
        const addedToSub = subAgent.receiveEvent(event);
        if (!added && !addedToSub) return;
        if (added && main.terminalEvent === event) liveLane.detach();
        if (added && options.mode === 'active' && event.type === 'run_failed'
            && eventPayload(event).userRetryable === true) {
            collapsed = false;
            selectedSeq = event.seq;
            detailsOpen = true;
        }
        publish();
        if (added && isTimelineProjectionStructuralEvent(event.type)) scheduleProjectionRefresh();
        if (added && detailsOpen && (selectedSeq == null || selectedSeq === event.seq) && eventShowsDetails(event)) {
            void loadDetails();
        }
    }

    function scheduleProjectionRefresh(): void {
        if (projectionTimer) clearTimeout(projectionTimer);
        projectionTimer = setTimeout(() => {
            projectionTimer = null;
            void refreshProjection();
        }, 120);
    }

    async function refreshProjection(): Promise<void> {
        try {
            await main.refreshProjection(deps.readEvents);
            publish();
        } catch (error) {
            deps.reportError(error);
        }
    }

    async function loadDetails(): Promise<void> {
        const selection = currentSelection(currentDerived());
        const item = selection.item;
        if (!item || !currentRun?.runId) {
            detail.reset();
            publish();
            return;
        }
        const pending = detail.load({
            runId: currentRun.runId,
            targets: selection.targets,
            readOnly: options.mode === 'history',
        });
        publish();
        await pending;
        publish();
    }

    async function savePanelHeight(height: number | null): Promise<void> {
        if (options.mode !== 'active' || !settings) return;
        settings = await options.deps.patchSettings(settings, { runTimelineHeightPx: height });
        panelHeightPx = normalizeRunTimelineHeightPx(settings.runTimelineHeightPx);
        publish();
    }

    async function init(): Promise<void> {
        initPromise ??= (async () => {
            if (options.mode === 'history') {
                await startTrackingRun(options.run);
                return;
            }
            try {
                unsubscribes.push(options.deps.subscribeSettings(next => {
                    settings = next;
                    panelHeightPx = normalizeRunTimelineHeightPx(next.runTimelineHeightPx);
                    publish();
                }));
                unsubscribes.push(options.deps.subscribeRunState(state => fire(handleRunState(state.activeRun, state.lastEvent))));
                unsubscribes.push(options.deps.subscribeRunEvents(event => receiveRunEvent(event)));
                settings = await options.deps.loadSettings();
                if (disposed) return;
                panelHeightPx = normalizeRunTimelineHeightPx(settings.runTimelineHeightPx);
                publish();
                await handleRunState(options.deps.getActiveRun(), null);
            } catch (error) {
                unsubscribes.splice(0).reverse().forEach(unsubscribe => unsubscribe());
                deps.reportError(error);
                throw error;
            }
        })();
        return initPromise;
    }

    const controller: RunTimelineController = {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => listeners.delete(listener);
        },
        init,
        dispose() {
            if (disposed) return;
            disposed = true;
            if (projectionTimer) clearTimeout(projectionTimer);
            projectionTimer = null;
            resizing = false;
            viewGesture = null;
            main.reset();
            detail.reset();
            subAgent.dispose();
            liveLane.dispose();
            unsubscribes.splice(0).reverse().forEach(unsubscribe => unsubscribe());
            listeners.clear();
        },
        async loadOlder() {
            try {
                const pending = main.loadOlder(deps.readEvents);
                publish();
                const applied = await pending;
                publish();
                return applied;
            } catch (error) {
                publish();
                deps.reportError(error);
                return false;
            }
        },
        selectItem(seq) {
            selectedSeq = seq;
            if (detailsOpen) void loadDetails();
            else publish();
        },
        toggleCollapsed() {
            collapsed = !collapsed;
            publish();
        },
        openDetails() {
            if (!snapshot.selectedHasDetails) return;
            detailsOpen = true;
            void loadDetails();
        },
        showTimeline() {
            detailsOpen = false;
            detail.reset();
            publish();
        },
        toggleSubAgentTray() {
            trayExpanded = !trayExpanded;
            publish();
        },
        openSubAgent(invocationId) {
            const normalized = invocationId.trim();
            if (!normalized) throw new Error('SubAgent invocationId is required.');
            if (!currentRun?.runId) throw new Error('Agent run id is required.');
            subAgent.open(currentRun.runId, normalized);
        },
        closeSubAgent() {
            subAgent.close();
        },
        async loadOlderSubAgent() {
            return subAgent.loadOlder();
        },
        selectSubAgentItem(seq) {
            subAgent.select(seq);
        },
        invokeDetailAction(action) {
            if (action.kind === 'openSubAgent') {
                controller.openSubAgent(action.invocationId);
            } else if (action.kind === 'retry' && options.mode === 'active') {
                fire(options.deps.retryFailure({ run: currentRun, events: main.events, terminalEvent: main.terminalEvent }));
            }
        },
        setTimelineViewport(next) {
            viewport = next;
            publish();
        },
        setSubAgentViewport(next) {
            subAgent.setViewport(next);
        },
        startViewGesture(event) {
            if (viewGesture || !canStartRunTimelineViewGesture({
                event,
                collapsed,
                resizing,
                detailsOpen,
                selectedHasDetails: snapshot.selectedHasDetails,
            })) return;
            viewGesture = createRunTimelineViewGesture(event, detailsOpen);
        },
        trackViewGesture(event) {
            if (shouldCancelRunTimelineViewGesture(viewGesture, event)) viewGesture = null;
        },
        finishViewGesture(event) {
            const gesture = viewGesture;
            if (!gesture || event.pointerId !== gesture.pointerId) return;
            viewGesture = null;
            const action = resolveRunTimelineViewGesture(gesture, event, {
                detailsOpen,
                selectedHasDetails: snapshot.selectedHasDetails,
            });
            if (action === RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS) controller.openDetails();
            else if (action === RUN_TIMELINE_VIEW_GESTURE_ACTION_TIMELINE) controller.showTimeline();
        },
        cancelViewGesture(pointerId) {
            if (viewGesture?.pointerId === pointerId) viewGesture = null;
        },
        startResize(startY, startHeight, bounds) {
            if (options.mode !== 'active' || collapsed) return;
            resizeBounds = bounds;
            resizeStartY = startY;
            resizeStartHeight = clampRunTimelineHeightPx(panelHeightPx ?? startHeight, bounds);
            panelHeightPx = resizeStartHeight;
            resizing = true;
            publish();
        },
        moveResize(clientY) {
            if (!resizing) return;
            panelHeightPx = heightFromTopEdgeDrag({
                startHeight: resizeStartHeight,
                startY: resizeStartY,
                currentY: clientY,
                bounds: resizeBounds,
            });
            publish();
        },
        finishResize(save) {
            if (!resizing) return;
            resizing = false;
            publish();
            if (save) fire(savePanelHeight(panelHeightPx));
        },
        resizeByKey(key, currentHeight, bounds) {
            if (options.mode !== 'active') return false;
            const current = clampRunTimelineHeightPx(panelHeightPx ?? currentHeight, bounds);
            const next = key === 'ArrowUp' ? current + RUN_TIMELINE_KEYBOARD_STEP_PX
                : key === 'ArrowDown' ? current - RUN_TIMELINE_KEYBOARD_STEP_PX
                    : key === 'PageUp' ? current + RUN_TIMELINE_PAGE_STEP_PX
                        : key === 'PageDown' ? current - RUN_TIMELINE_PAGE_STEP_PX
                            : key === 'Home' ? bounds.min : key === 'End' ? bounds.max : null;
            if (next == null) return false;
            panelHeightPx = clampRunTimelineHeightPx(next, bounds);
            publish();
            fire(savePanelHeight(panelHeightPx));
            return true;
        },
        resetPanelHeight() {
            if (options.mode === 'active') fire(savePanelHeight(null));
        },
        requestClose() {
            if (options.mode === 'history') options.requestClose();
        },
    };
    return controller;
}

function eventPayload(event: TauriTavernAgentRunEvent): Record<string, unknown> {
    return event.payload && typeof event.payload === 'object' && !Array.isArray(event.payload)
        ? event.payload as Record<string, unknown>
        : {};
}
