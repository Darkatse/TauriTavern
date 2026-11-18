import {
    memo,
    useCallback,
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
    useSyncExternalStore,
    type CSSProperties,
    type KeyboardEvent as ReactKeyboardEvent,
    type PointerEvent as ReactPointerEvent,
    type Ref,
    type RefObject,
} from 'react';

import type { AgentSystemTr } from './i18n';
import type { RunTimelineController, RunTimelineSnapshot, SubAgentTask, TimelineItem } from './RunTimelineContract';
import {
    RunTimelineDetailPane,
    RunTimelineEventList,
    SubAgentTray,
} from './RunTimelineComponents';
import {
    captureTimelineScrollAnchor,
    observeTimelineHeight,
    readTimelineHeightBounds,
    restoreTimelineScrollAnchor,
    type TimelineScrollAnchor,
} from './RunTimelineDom';
import { SubAgentTimelineDialog } from './SubAgentTimelineDialog';

type TimelinePanelStyle = CSSProperties & {
    '--ttas-run-panel-user-height'?: string;
    '--ttas-run-panel-available-height'?: string;
};

const INDICATOR_BEAT_MS = 175;

export function RunTimelineApp(props: { controller: RunTimelineController; tr: AgentSystemTr }) {
    const { controller, tr } = props;
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const panelRef = useRef<HTMLElement>(null);
    const headerRef = useRef<HTMLElement>(null);
    const bodyRef = useRef<HTMLDivElement>(null);
    const scrollerRef = useRef<HTMLDivElement>(null);
    const anchorRef = useRef<TimelineScrollAnchor | null>(null);
    const [anchorRevision, setAnchorRevision] = useState(0);
    const [availableHeight, setAvailableHeight] = useState<number | null>(null);

    useLayoutEffect(() => {
        if (snapshot.mode !== 'active' || !snapshot.visible || snapshot.collapsed) return;
        const panel = panelRef.current;
        const header = headerRef.current;
        if (!panel || !header) throw new Error('Agent run timeline layout elements are unavailable.');
        return observeTimelineHeight(panel, header, setAvailableHeight);
    }, [snapshot.mode, snapshot.visible, snapshot.collapsed]);

    useLayoutEffect(() => {
        restoreTimelineScrollAnchor(scrollerRef.current, anchorRef.current, controller.setTimelineViewport);
        anchorRef.current = null;
    }, [anchorRevision, controller]);

    useIndicatorBeat(panelRef, snapshot.visible && snapshot.panelView === 'events' && snapshot.isRunning);

    useEffect(() => {
        const body = bodyRef.current;
        if (!body) return;
        const cancel = (event: PointerEvent) => controller.cancelViewGesture(event.pointerId);
        body.addEventListener('pointerdown', controller.startViewGesture, { passive: true });
        body.addEventListener('pointermove', controller.trackViewGesture, { passive: true });
        body.addEventListener('pointerup', controller.finishViewGesture, { passive: true });
        body.addEventListener('pointercancel', cancel, { passive: true });
        return () => {
            body.removeEventListener('pointerdown', controller.startViewGesture);
            body.removeEventListener('pointermove', controller.trackViewGesture);
            body.removeEventListener('pointerup', controller.finishViewGesture);
            body.removeEventListener('pointercancel', cancel);
        };
    }, [controller, snapshot.collapsed]);

    const loadOlder = useCallback(async () => {
        const anchor = captureTimelineScrollAnchor(scrollerRef.current);
        if (await controller.loadOlder()) {
            anchorRef.current = anchor;
            setAnchorRevision(revision => revision + 1);
        }
    }, [controller]);
    const onTopReached = useCallback(() => { void loadOlder(); }, [loadOlder]);
    const selectItem = useCallback((item: TimelineItem) => controller.selectItem(item.seq), [controller]);
    const toggleLiveItem = useCallback((item: TimelineItem) => controller.toggleLiveItem(item.id), [controller]);
    const openSubAgent = useCallback((task: SubAgentTask) => controller.openSubAgent(task.targetInvocationId), [controller]);

    function resizeBounds() {
        const panel = panelRef.current;
        const header = headerRef.current;
        if (!panel || !header) {
            throw new Error('Agent run timeline resize elements are unavailable.');
        }
        return readTimelineHeightBounds(panel, header);
    }

    function currentPanelHeight(): number {
        const body = bodyRef.current;
        if (!body) throw new Error('Agent run timeline body is unavailable.');
        return Math.round(body.getBoundingClientRect().height);
    }

    function startResize(event: ReactPointerEvent<HTMLButtonElement>): void {
        event.preventDefault();
        controller.startResize(event.clientY, currentPanelHeight(), resizeBounds());
        event.currentTarget.setPointerCapture(event.pointerId);
    }

    function resizeByKey(event: ReactKeyboardEvent<HTMLButtonElement>): void {
        if (controller.resizeByKey(event.key, currentPanelHeight(), resizeBounds())) event.preventDefault();
    }

    const panelStyle: TimelinePanelStyle = {};
    if (!snapshot.visible) panelStyle.display = 'none';
    if (snapshot.mode === 'active' && snapshot.panelHeightPx != null) {
        panelStyle['--ttas-run-panel-user-height'] = `${snapshot.panelHeightPx}px`;
    }
    if (snapshot.mode === 'active' && availableHeight != null) {
        panelStyle['--ttas-run-panel-available-height'] = `${availableHeight}px`;
    }
    const classes = [
        'ttas-root ttas-run-panel',
        snapshot.collapsed && 'is-collapsed',
        snapshot.mode === 'history' && 'is-history',
        snapshot.isRunning && 'is-running',
        snapshot.detailsOpen && 'is-details-open',
        snapshot.terminalType && 'is-terminal',
        snapshot.terminalType === 'run_failed' && 'is-error',
        snapshot.terminalType === 'run_partial_success' && 'is-warning',
        snapshot.resizing && 'is-resizing',
    ].filter(Boolean).join(' ');

    return (
        <section
            ref={panelRef}
            id={snapshot.rootId}
            className={classes}
            data-ttas-status={snapshot.panelStatus}
            data-ttas-view={snapshot.panelView}
            style={panelStyle}
            aria-live="polite"
        >
            {snapshot.mode === 'active' && !snapshot.collapsed && (
                <button
                    type="button"
                    className="ttas-run-resize-handle"
                    title={tr('resizeTimelineHeight')}
                    aria-label={tr('resizeTimelineHeight')}
                    role="separator"
                    aria-orientation="horizontal"
                    onPointerDown={startResize}
                    onPointerMove={event => controller.moveResize(event.clientY)}
                    onPointerUp={() => controller.finishResize(true)}
                    onPointerCancel={() => controller.finishResize(false)}
                    onDoubleClick={controller.resetPanelHeight}
                    onKeyDown={resizeByKey}
                ></button>
            )}
            <RunTimelineHeader
                headerRef={headerRef}
                controller={controller}
                tr={tr}
                mode={snapshot.mode}
                title={snapshot.headerTitle}
                subtitle={snapshot.headerSubtitle}
                subtitleLive={Boolean(snapshot.displayItems.at(-1)?.live)}
                collapsed={snapshot.collapsed}
                detailsOpen={snapshot.detailsOpen}
                canOpenDetails={Boolean(snapshot.selectedItem) && snapshot.selectedHasDetails}
            />

            {!snapshot.collapsed && (
                <div ref={bodyRef} className="ttas-run-body">
                    {snapshot.presentationError && (
                        <div className="ttas-run-detail-error" role="alert">
                            <span>{tr('timelinePresentationSaveFailed')}: {snapshot.presentationError}</span>
                            <button type="button" className="menu_button" disabled={snapshot.savingPresentation}
                                onClick={() => void controller.retryPresentation()}>
                                {tr('timelineRetryPresentation')}
                            </button>
                        </div>
                    )}
                    <section
                        className="ttas-run-view ttas-run-view-events"
                        style={{ display: snapshot.detailsOpen ? 'none' : undefined }}
                        aria-label={tr('agentTimeline')}
                    >
                        <RunTimelineEventList
                            tr={tr}
                            scrollerRef={scrollerRef}
                            ariaLabel={tr('agentTimeline')}
                            surfaceClass="ttas-run-event-scroll"
                            loading={snapshot.loading}
                            loadingOlder={snapshot.loadingOlder}
                            emptyText={snapshot.emptyText}
                            items={snapshot.displayItems}
                            virtualItems={snapshot.virtualItems}
                            liveItems={snapshot.liveItems}
                            onToggleLive={toggleLiveItem}
                            selectedSeq={snapshot.selectedSeq}
                            latestSeq={snapshot.latestSeq}
                            activeSeq={snapshot.activeSeq}
                            autoStick={snapshot.autoStick}
                            onSelect={selectItem}
                            onTopReached={onTopReached}
                            onViewport={controller.setTimelineViewport}
                        />
                        <SubAgentTray
                            tr={tr}
                            expanded={snapshot.trayExpanded}
                            tasks={snapshot.subAgentTasks}
                            title={snapshot.subAgentTrayTitle}
                            onToggle={controller.toggleSubAgentTray}
                            onSelect={openSubAgent}
                        />
                    </section>
                    {snapshot.detailsOpen && (
                        <RunTimelineDetailPane
                            tr={tr}
                            rootClass="ttas-run-view ttas-run-view-details"
                            ariaLabel={tr('timelineDetails')}
                            title={snapshot.detailTitle}
                            type={snapshot.selectedItem?.type ?? ''}
                            items={snapshot.displayItems}
                            hasMoreBefore={snapshot.hasMoreBefore}
                            loadingOlder={snapshot.loadingOlder}
                            onLoadOlder={controller.loadOlder}
                            selectedSeq={snapshot.selectedSeq}
                            loading={snapshot.detail.loading}
                            error={snapshot.detail.error}
                            sections={snapshot.detail.sections}
                            showBack
                            onBack={controller.showTimeline}
                            onSelectNav={selectItem}
                            onAction={controller.invokeDetailAction}
                        />
                    )}
                </div>
            )}
            <SubAgentTimelineDialog controller={controller} snapshot={snapshot.subAgent} tr={tr} />
        </section>
    );
}

const RunTimelineHeader = memo(function RunTimelineHeader({
    headerRef, controller, tr, mode, title, subtitle, subtitleLive, collapsed, detailsOpen, canOpenDetails,
}: {
    headerRef: Ref<HTMLElement>;
    controller: RunTimelineController;
    tr: AgentSystemTr;
    mode: RunTimelineSnapshot['mode'];
    title: string;
    subtitle: string;
    subtitleLive: boolean;
    collapsed: boolean;
    detailsOpen: boolean;
    canOpenDetails: boolean;
}) {
    return (
        <header ref={headerRef} className="ttas-run-header">
            <div className="ttas-run-heading">
                <span className="ttas-run-orb" aria-hidden="true">
                    <i className="fa-solid fa-wand-magic-sparkles"></i>
                </span>
                <div className="ttas-run-heading-copy">
                    <strong>{title}</strong>
                    <small aria-live={subtitleLive ? 'off' : undefined}>
                        {subtitle}
                    </small>
                </div>
            </div>
            <div className="ttas-run-actions">
                <button
                    type="button"
                    className="menu_button menu_button_icon ttas-run-icon-button"
                    title={tr(detailsOpen ? 'showTimelineEvents' : 'showTimelineDetails')}
                    aria-label={tr(detailsOpen ? 'showTimelineEvents' : 'showTimelineDetails')}
                    disabled={collapsed || (!detailsOpen && !canOpenDetails)}
                    onClick={detailsOpen ? controller.showTimeline : controller.openDetails}
                >
                    <i className={`fa-solid ${detailsOpen ? 'fa-list' : 'fa-circle-info'}`}></i>
                </button>
                {mode === 'history' ? (
                    <button
                        type="button"
                        className="menu_button menu_button_icon ttas-run-icon-button"
                        title={tr('close')}
                        aria-label={tr('close')}
                        onClick={controller.requestClose}
                    >
                        <i className="fa-solid fa-xmark"></i>
                    </button>
                ) : (
                    <button
                        type="button"
                        className="menu_button menu_button_icon ttas-run-icon-button"
                        title={tr(collapsed ? 'expandTimeline' : 'collapseTimeline')}
                        aria-label={tr(collapsed ? 'expandTimeline' : 'collapseTimeline')}
                        aria-expanded={!collapsed}
                        onClick={controller.toggleCollapsed}
                    >
                        <i className={`fa-solid ${collapsed ? 'fa-chevron-up' : 'fa-chevron-down'}`}></i>
                    </button>
                )}
            </div>
        </header>
    );
});

/**
 * Steps the panel's looping indicators: while enabled, the page is visible and motion is not reduced, the panel
 * carries the beat count modulo 6 and 9 as data-ttas-beat6 and data-ttas-beat9. Otherwise it carries neither, and
 * the indicators rest.
 */
function useIndicatorBeat(panelRef: RefObject<HTMLElement | null>, enabled: boolean): void {
    useEffect(() => {
        const panel = panelRef.current;
        if (!enabled || !panel) return;
        const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
        let timer: ReturnType<typeof setInterval> | null = null;
        let beat = 0;
        const step = () => {
            beat += 1;
            panel.dataset.ttasBeat6 = String(beat % 6);
            panel.dataset.ttasBeat9 = String(beat % 9);
        };
        const stop = () => {
            if (timer !== null) clearInterval(timer);
            timer = null;
            delete panel.dataset.ttasBeat6;
            delete panel.dataset.ttasBeat9;
        };
        const sync = () => {
            if (document.visibilityState !== 'visible' || reducedMotion.matches) {
                stop();
            } else if (timer === null) {
                step();
                timer = setInterval(step, INDICATOR_BEAT_MS);
            }
        };
        sync();
        document.addEventListener('visibilitychange', sync);
        reducedMotion.addEventListener('change', sync);
        return () => {
            document.removeEventListener('visibilitychange', sync);
            reducedMotion.removeEventListener('change', sync);
            stop();
        };
    }, [enabled, panelRef]);
}
