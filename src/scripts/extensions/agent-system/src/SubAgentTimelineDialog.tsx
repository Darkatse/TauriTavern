import {
    useCallback,
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
    type CSSProperties,
} from 'react';

import type { AgentSystemTr } from './i18n.js';
import type {
    RunTimelineController,
    SubAgentTimelineSnapshot,
} from './RunTimelineContract';
import {
    RunTimelineDetailPane,
    RunTimelineEventList,
} from './RunTimelineComponents';
import {
    captureTimelineScrollAnchor,
    restoreTimelineScrollAnchor,
    scrollTimelineToBottom,
    type TimelineScrollAnchor,
} from './RunTimelineDom';
import { subAgentTaskStyle, timelineItemTitle } from './run-timeline-display';

export function SubAgentTimelineDialog(props: {
    controller: RunTimelineController;
    snapshot: SubAgentTimelineSnapshot;
    tr: AgentSystemTr;
}) {
    const { controller, snapshot, tr } = props;
    const dialogRef = useRef<HTMLDialogElement>(null);
    const scrollerRef = useRef<HTMLDivElement>(null);
    const anchorRef = useRef<TimelineScrollAnchor | null>(null);
    const [anchorRevision, setAnchorRevision] = useState(0);

    useLayoutEffect(() => {
        const dialog = dialogRef.current;
        if (!dialog) return;
        if (snapshot.open && !dialog.open) {
            try {
                dialog.showModal();
            } catch (error) {
                controller.closeSubAgent();
                throw error;
            }
        } else if (!snapshot.open && dialog.open) {
            dialog.close();
        }
    }, [controller, snapshot.open]);

    useEffect(() => () => {
        const dialog = dialogRef.current;
        if (dialog?.open) dialog.close();
    }, []);

    useLayoutEffect(() => {
        if (snapshot.open && snapshot.autoStick) {
            scrollTimelineToBottom(scrollerRef.current, controller.setSubAgentViewport);
        }
    }, [controller, snapshot.autoStick, snapshot.displayItems, snapshot.open]);

    useLayoutEffect(() => {
        restoreTimelineScrollAnchor(scrollerRef.current, anchorRef.current, controller.setSubAgentViewport);
        anchorRef.current = null;
    }, [anchorRevision, controller]);

    const loadOlder = useCallback(async () => {
        const anchor = captureTimelineScrollAnchor(scrollerRef.current);
        if (await controller.loadOlderSubAgent()) {
            anchorRef.current = anchor;
            setAnchorRevision(revision => revision + 1);
        }
    }, [controller]);
    const onTopReached = useCallback(() => { void loadOlder(); }, [loadOlder]);

    const close = () => dialogRef.current?.close();
    const taskStyle = snapshot.task ? subAgentTaskStyle(snapshot.task) as CSSProperties : undefined;

    return (
        <dialog
            ref={dialogRef}
            className="ttas-dialog ttas-subagent-dialog"
            data-tt-mobile-surface="fullscreen-window"
            onCancel={(event) => {
                event.preventDefault();
                close();
            }}
            onClose={controller.closeSubAgent}
        >
            <div className="ttas-subagent-panel">
                <header className="ttas-subagent-titlebar">
                    <div className="ttas-subagent-title">
                        <span className="ttas-subagent-title-dot" style={taskStyle} aria-hidden="true"></span>
                        <div>
                            <strong>{snapshot.title}</strong>
                            <small>{snapshot.subtitle}</small>
                        </div>
                    </div>
                    <button
                        type="button"
                        className="menu_button menu_button_icon ttas-run-icon-button"
                        title={tr('close')}
                        aria-label={tr('close')}
                        onClick={close}
                    >
                        <i className="fa-solid fa-xmark"></i>
                    </button>
                </header>
                <div className="ttas-subagent-body">
                    <RunTimelineEventList
                        tr={tr}
                        scrollerRef={scrollerRef}
                        ariaLabel={tr('timelineSubAgentTimeline')}
                        surfaceClass="ttas-subagent-timeline"
                        listClass="ttas-subagent-events"
                        loading={snapshot.loading}
                        loadingOlder={snapshot.loadingOlder}
                        emptyText={tr('timelineNoEvents')}
                        items={snapshot.displayItems}
                        virtualItems={snapshot.virtualItems}
                        selectedSeq={snapshot.selectedSeq}
                        latestSeq={null}
                        activeSeq={null}
                        onSelect={item => controller.selectSubAgentItem(item.seq)}
                        onTopReached={onTopReached}
                        onViewport={controller.setSubAgentViewport}
                    />
                    <RunTimelineDetailPane
                        tr={tr}
                        rootClass="ttas-subagent-detail"
                        ariaLabel={tr('timelineDetails')}
                        title={snapshot.selectedItem ? timelineItemTitle(snapshot.selectedItem, tr) : tr('timelineDetails')}
                        type={snapshot.selectedItem?.type ?? ''}
                        navItems={snapshot.navItems}
                        selectedSeq={snapshot.selectedSeq}
                        loading={snapshot.detail.loading}
                        error={snapshot.detail.error}
                        sections={snapshot.detail.sections}
                        onSelectNav={item => controller.selectSubAgentItem(item.seq)}
                        onAction={controller.invokeDetailAction}
                    />
                </div>
            </div>
        </dialog>
    );
}
