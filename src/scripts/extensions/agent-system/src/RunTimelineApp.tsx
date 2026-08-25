import {
    useCallback,
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
    useSyncExternalStore,
    type CSSProperties,
    type KeyboardEvent as ReactKeyboardEvent,
    type PointerEvent as ReactPointerEvent,
} from 'react';

import type { AgentSystemTr } from './i18n';
import type { RunTimelineController } from './RunTimelineContract';
import {
    RunTimelineDetailPane,
    RunTimelineEventList,
    SubAgentTray,
} from './RunTimelineComponents';
import {
    captureTimelineScrollAnchor,
    readTimelineViewport,
    restoreTimelineScrollAnchor,
    scrollTimelineToBottom,
    type TimelineScrollAnchor,
} from './RunTimelineDom';
import { runTimelineHeightBounds } from './run-timeline-resize';
import { SubAgentTimelineDialog } from './SubAgentTimelineDialog';

type TimelinePanelStyle = CSSProperties & { '--ttas-run-panel-user-height'?: string };

export function RunTimelineApp(props: { controller: RunTimelineController; tr: AgentSystemTr }) {
    const { controller, tr } = props;
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const panelRef = useRef<HTMLElement>(null);
    const headerRef = useRef<HTMLElement>(null);
    const bodyRef = useRef<HTMLDivElement>(null);
    const scrollerRef = useRef<HTMLDivElement>(null);
    const anchorRef = useRef<TimelineScrollAnchor | null>(null);
    const [anchorRevision, setAnchorRevision] = useState(0);

    useLayoutEffect(() => {
        restoreTimelineScrollAnchor(scrollerRef.current, anchorRef.current, controller.setTimelineViewport);
        anchorRef.current = null;
    }, [anchorRevision, controller]);

    useLayoutEffect(() => {
        const scroller = scrollerRef.current;
        if (snapshot.collapsed || snapshot.detailsOpen || !scroller) return;
        if (snapshot.autoStick) scrollTimelineToBottom(scroller, controller.setTimelineViewport);
        else controller.setTimelineViewport(readTimelineViewport(scroller));
    }, [controller, snapshot.autoStick, snapshot.collapsed, snapshot.detailsOpen, snapshot.displayItems]);

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

    function resizeBounds() {
        const panel = panelRef.current;
        const header = headerRef.current;
        if (!panel || !header) {
            throw new Error('Agent run timeline resize elements are unavailable.');
        }
        const topBar = document.getElementById('top-bar');
        const viewportTop = window.visualViewport?.offsetTop || 0;
        return runTimelineHeightBounds({
            panelBottom: panel.getBoundingClientRect().bottom,
            topBoundary: Math.max(
                viewportTop,
                topBar instanceof HTMLElement ? topBar.getBoundingClientRect().bottom : 0,
            ),
            chromeHeight: header.getBoundingClientRect().height,
        });
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
            <header ref={headerRef} className="ttas-run-header">
                <div className="ttas-run-heading">
                    <span className="ttas-run-orb" aria-hidden="true">
                        <i className="fa-solid fa-wand-magic-sparkles"></i>
                    </span>
                    <div className="ttas-run-heading-copy">
                        <strong>{snapshot.headerTitle}</strong>
                        <small>{snapshot.headerSubtitle}</small>
                    </div>
                </div>
                <div className="ttas-run-actions">
                    <button
                        type="button"
                        className="menu_button menu_button_icon ttas-run-icon-button"
                        title={tr(snapshot.detailsOpen ? 'showTimelineEvents' : 'showTimelineDetails')}
                        aria-label={tr(snapshot.detailsOpen ? 'showTimelineEvents' : 'showTimelineDetails')}
                        disabled={snapshot.collapsed || (!snapshot.detailsOpen
                            && (!snapshot.selectedItem || !snapshot.selectedHasDetails))}
                        onClick={snapshot.detailsOpen ? controller.showTimeline : controller.openDetails}
                    >
                        <i className={`fa-solid ${snapshot.detailsOpen ? 'fa-list' : 'fa-circle-info'}`}></i>
                    </button>
                    {snapshot.mode === 'history' ? (
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
                            title={tr(snapshot.collapsed ? 'expandTimeline' : 'collapseTimeline')}
                            aria-label={tr(snapshot.collapsed ? 'expandTimeline' : 'collapseTimeline')}
                            aria-expanded={!snapshot.collapsed}
                            onClick={controller.toggleCollapsed}
                        >
                            <i className={`fa-solid ${snapshot.collapsed ? 'fa-chevron-up' : 'fa-chevron-down'}`}></i>
                        </button>
                    )}
                </div>
            </header>

            {!snapshot.collapsed && (
                <div ref={bodyRef} className="ttas-run-body">
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
                            selectedSeq={snapshot.selectedSeq}
                            latestSeq={snapshot.latestSeq}
                            activeSeq={snapshot.activeSeq}
                            onSelect={item => controller.selectItem(item.seq)}
                            onTopReached={onTopReached}
                            onViewport={controller.setTimelineViewport}
                        />
                        <SubAgentTray
                            tr={tr}
                            expanded={snapshot.trayExpanded}
                            tasks={snapshot.subAgentTasks}
                            title={snapshot.subAgentTrayTitle}
                            onToggle={controller.toggleSubAgentTray}
                            onSelect={task => controller.openSubAgent(task.targetInvocationId)}
                        />
                    </section>
                    {snapshot.detailsOpen && (
                        <RunTimelineDetailPane
                            tr={tr}
                            rootClass="ttas-run-view ttas-run-view-details"
                            ariaLabel={tr('timelineDetails')}
                            title={snapshot.detailTitle}
                            type={snapshot.selectedItem?.type ?? ''}
                            navItems={snapshot.navItems}
                            selectedSeq={snapshot.selectedSeq}
                            loading={snapshot.detail.loading}
                            error={snapshot.detail.error}
                            sections={snapshot.detail.sections}
                            showBack
                            onBack={controller.showTimeline}
                            onSelectNav={item => controller.selectItem(item.seq)}
                            onAction={controller.invokeDetailAction}
                        />
                    )}
                </div>
            )}
            <SubAgentTimelineDialog controller={controller} snapshot={snapshot.subAgent} tr={tr} />
        </section>
    );
}
