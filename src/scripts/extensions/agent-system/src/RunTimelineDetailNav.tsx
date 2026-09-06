import { useCallback, useLayoutEffect, useRef, useState } from 'react';

import type { AgentSystemTr } from './i18n';
import type { TimelineItem } from './RunTimelineContract';
import { timelineItemShortLabel, timelineItemTitle } from './run-timeline-display';

const ROW_HEIGHT = 32;
const GAP = 5;
const MIN_ITEM_WIDTH = 80;
const OVERSCAN_ROWS = 3;

export function RunTimelineDetailNav(props: {
    items: readonly TimelineItem[];
    selectedSeq: number | null;
    hasMoreBefore: boolean;
    loadingOlder: boolean;
    onLoadOlder: () => Promise<boolean>;
    onSelect: (item: TimelineItem) => void;
    tr: AgentSystemTr;
}) {
    const { items, selectedSeq, hasMoreBefore } = props;
    const scrollerRef = useRef<HTMLDivElement>(null);
    const anchorRef = useRef<{ id: string; offset: number } | null>(null);
    const programmaticTopRef = useRef<number | null>(null);
    const [viewport, setViewport] = useState({ top: 0, width: 0, height: 0 });
    const columns = Math.max(1, Math.floor((viewport.width + GAP) / (MIN_ITEM_WIDTH + GAP)));
    const leadingRows = hasMoreBefore ? 1 : 0;
    const rowCount = leadingRows + Math.ceil(items.length / columns);
    const firstRow = Math.max(0, Math.min(rowCount - 1, Math.floor(viewport.top / ROW_HEIGHT)) - OVERSCAN_ROWS);
    const lastRow = Math.min(rowCount, Math.ceil((viewport.top + viewport.height) / ROW_HEIGHT) + OVERSCAN_ROWS);
    const start = Math.max(0, firstRow - leadingRows) * columns;
    const end = Math.max(0, lastRow - leadingRows) * columns;
    const previous = useRef<{
        items: readonly TimelineItem[];
        selectedSeq: number | null;
        columns: number;
        leadingRows: number;
    } | null>(null);

    const measure = useCallback(() => {
        const scroller = scrollerRef.current;
        if (!scroller) return;
        const next = { top: scroller.scrollTop, width: scroller.clientWidth, height: scroller.clientHeight };
        setViewport(current => current.top === next.top && current.width === next.width && current.height === next.height
            ? current : next);
    }, []);

    useLayoutEffect(() => {
        const scroller = scrollerRef.current;
        if (!scroller) return;
        measure();
        const observer = new ResizeObserver(measure);
        observer.observe(scroller);
        return () => observer.disconnect();
    }, [measure]);

    useLayoutEffect(() => {
        const scroller = scrollerRef.current;
        if (!scroller) return;
        const before = previous.current;
        const selectionChanged = !anchorRef.current || (before && selectedSeq !== before.selectedSeq && items === before.items);
        if (selectionChanged) {
            const selected = items.find(item => item.seq === selectedSeq);
            anchorRef.current = selected ? { id: selected.id, offset: 0 } : null;
        }
        if (selectionChanged || !before || columns !== before.columns
            || leadingRows !== before.leadingRows || items[0]?.id !== before.items[0]?.id) {
            const anchor = anchorRef.current;
            const index = anchor ? items.findIndex(item => item.id === anchor.id) : -1;
            if (index >= 0) {
                scroller.scrollTop = (leadingRows + Math.floor(index / columns)) * ROW_HEIGHT - (anchor?.offset ?? 0);
                programmaticTopRef.current = scroller.scrollTop;
            }
        }
        previous.current = { items, selectedSeq, columns, leadingRows };
        measure();
    }, [items, selectedSeq, columns, leadingRows, measure]);

    return (
        <div className="ttas-run-detail-nav">
            <div
                ref={scrollerRef}
                className="ttas-run-nav-list"
                role="navigation"
                aria-label={props.tr('timelineDetails')}
                aria-busy={props.loadingOlder}
                onScroll={() => {
                    const scroller = scrollerRef.current;
                    if (!scroller) return;
                    // Keep one event anchor through every frame of a width transition.
                    if (scroller.scrollTop === programmaticTopRef.current) {
                        programmaticTopRef.current = null;
                    } else {
                        const row = Math.max(0, Math.floor(scroller.scrollTop / ROW_HEIGHT) - leadingRows);
                        const item = items[row * columns];
                        anchorRef.current = item
                            ? { id: item.id, offset: (row + leadingRows) * ROW_HEIGHT - scroller.scrollTop } : null;
                    }
                    measure();
                    if (scroller.scrollTop <= ROW_HEIGHT
                        && hasMoreBefore && !props.loadingOlder) void props.onLoadOlder();
                }}
            >
                <div className="ttas-run-nav-space" style={{ height: Math.max(0, rowCount * ROW_HEIGHT - GAP) }}>
                    <div
                        className="ttas-run-nav-window"
                        style={{
                            top: firstRow * ROW_HEIGHT,
                            gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                            gridAutoRows: ROW_HEIGHT - GAP,
                            gap: GAP,
                        }}
                    >
                        {leadingRows > 0 && firstRow === 0 && (
                            <button
                                type="button"
                                className="ttas-run-nav-load"
                                disabled={props.loadingOlder}
                                onClick={() => { void props.onLoadOlder(); }}
                            >
                                {props.tr(props.loadingOlder ? 'timelineLoading' : 'runHistoryLoadMore')}
                            </button>
                        )}
                        {items.slice(start, end).map(item => (
                            <button
                                key={item.id}
                                type="button"
                                className={selectedSeq === item.seq ? 'is-selected' : ''}
                                aria-current={selectedSeq === item.seq ? 'step' : undefined}
                                title={timelineItemTitle(item, props.tr)}
                                onClick={(event) => {
                                    event.stopPropagation();
                                    props.onSelect(item);
                                }}
                            >
                                <i aria-hidden="true"></i>
                                <span>{timelineItemShortLabel(item, props.tr)}</span>
                            </button>
                        ))}
                    </div>
                </div>
            </div>
        </div>
    );
}
