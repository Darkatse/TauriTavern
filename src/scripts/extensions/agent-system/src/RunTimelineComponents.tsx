import { useEffect, type CSSProperties, type RefObject } from 'react';

import type { AgentSystemTr } from './i18n';
import type {
    SubAgentTask,
    TimelineDetailAction,
    TimelineDetailSection,
    TimelineItem,
    TimelineViewport,
    TimelineVirtualWindow,
} from './RunTimelineContract';
import { readTimelineViewport } from './RunTimelineDom';
import {
    subAgentStatusLabel,
    subAgentTaskStyle,
    subAgentTaskTone,
    timelineItemShortLabel,
    timelineItemTime,
    timelineItemTitle,
} from './run-timeline-display';
import { timelineItemHeightPx, timelineItemRowSpan } from './run-timeline-virtual-list';

const HISTORY_TOP_LOAD_THRESHOLD_PX = 72;

export type TimelineEventListProps = {
    tr: AgentSystemTr;
    scrollerRef: RefObject<HTMLDivElement | null>;
    ariaLabel: string;
    surfaceClass: string;
    listClass?: string;
    loading: boolean;
    loadingOlder: boolean;
    emptyText: string;
    items: readonly TimelineItem[];
    virtualItems: TimelineVirtualWindow;
    selectedSeq: number | null;
    latestSeq: number | null;
    activeSeq: number | null;
    onSelect: (item: TimelineItem) => void;
    onTopReached: () => void;
    onViewport: (viewport: TimelineViewport) => void;
};

export function RunTimelineEventList(props: TimelineEventListProps) {
    const {
        scrollerRef,
        onTopReached,
        onViewport,
    } = props;

    useEffect(() => {
        const scroller = scrollerRef.current;
        if (!scroller) return;
        const onScroll = () => {
            onViewport(readTimelineViewport(scroller));
            if (scroller.scrollTop <= HISTORY_TOP_LOAD_THRESHOLD_PX) onTopReached();
        };
        scroller.addEventListener('scroll', onScroll, { passive: true });
        return () => scroller.removeEventListener('scroll', onScroll);
    }, [onTopReached, onViewport, scrollerRef]);

    return (
        <div ref={scrollerRef} className={props.surfaceClass} aria-label={props.ariaLabel}>
            {props.loading && props.items.length === 0 ? (
                <div className="ttas-run-empty">
                    <i className="fa-solid fa-spinner fa-spin"></i>
                    <span>{props.tr('timelineLoading')}</span>
                </div>
            ) : props.items.length === 0 ? (
                <div className="ttas-run-empty">
                    <i className="fa-solid fa-circle-dot"></i>
                    <span>{props.emptyText}</span>
                </div>
            ) : (
                <ol className={`ttas-run-events is-windowed${props.listClass ? ` ${props.listClass}` : ''}`}>
                    {props.loadingOlder && (
                        <li className="ttas-run-event-loader" aria-live="polite">
                            <i className="fa-solid fa-spinner fa-spin"></i>
                            <span>{props.tr('timelineLoading')}</span>
                        </li>
                    )}
                    {props.virtualItems.topPadding > 0 && (
                        <li
                            className="ttas-run-event-spacer"
                            style={{ height: props.virtualItems.topPadding }}
                            aria-hidden="true"
                        ></li>
                    )}
                    {props.virtualItems.items.map(item => {
                        const selected = props.selectedSeq != null && props.selectedSeq === item.seq;
                        const latest = props.latestSeq != null && props.latestSeq === item.seq;
                        const active = props.activeSeq != null && props.activeSeq === item.seq;
                        const time = timelineItemTime(item);
                        const style = {
                            '--ttas-run-event-item-height': `${timelineItemHeightPx(item)}px`,
                        } as CSSProperties;
                        return (
                            <li
                                key={item.id}
                                className={[
                                    'ttas-run-event',
                                    `tone-${item.tone}`,
                                    `kind-${item.kind}`,
                                    latest && 'is-latest',
                                    active && 'is-active',
                                    selected && 'is-selected',
                                ].filter(Boolean).join(' ')}
                                data-ttas-kind={item.kind}
                                data-ttas-row-span={timelineItemRowSpan(item)}
                                style={style}
                            >
                                <button type="button" onClick={() => props.onSelect(item)}>
                                    <span className="ttas-run-event-icon" aria-hidden="true">
                                        <i className={`fa-solid ${item.icon}`}></i>
                                    </span>
                                    <span className="ttas-run-event-copy">
                                        <span className="ttas-run-event-title">
                                            {timelineItemTitle(item, props.tr)}
                                            {active && (
                                                <span className="ttas-run-ellipsis" aria-hidden="true">
                                                    <i>.</i><i>.</i><i>.</i>
                                                </span>
                                            )}
                                        </span>
                                        {item.summary && <small>{item.summary}</small>}
                                    </span>
                                    <span className="ttas-run-event-meta">
                                        <em>{timelineItemShortLabel(item, props.tr)}</em>
                                        {time && <time>{time}</time>}
                                    </span>
                                </button>
                            </li>
                        );
                    })}
                    {props.virtualItems.bottomPadding > 0 && (
                        <li
                            className="ttas-run-event-spacer"
                            style={{ height: props.virtualItems.bottomPadding }}
                            aria-hidden="true"
                        ></li>
                    )}
                </ol>
            )}
        </div>
    );
}

export type TimelineDetailPaneProps = {
    tr: AgentSystemTr;
    rootClass: string;
    ariaLabel: string;
    title: string;
    type: string;
    navItems: readonly TimelineItem[];
    selectedSeq: number | null;
    loading: boolean;
    error: string;
    sections: readonly TimelineDetailSection[];
    showBack?: boolean;
    onBack?: () => void;
    onSelectNav: (item: TimelineItem) => void;
    onAction: (action: TimelineDetailAction) => void;
};

export function RunTimelineDetailPane(props: TimelineDetailPaneProps) {
    return (
        <section className={props.rootClass} aria-label={props.ariaLabel}>
            <div className="ttas-run-detail-head">
                {props.showBack && (
                    <button
                        type="button"
                        className="menu_button menu_button_icon ttas-run-icon-button"
                        title={props.tr('showTimelineEvents')}
                        aria-label={props.tr('showTimelineEvents')}
                        onClick={props.onBack}
                    >
                        <i className="fa-solid fa-arrow-left"></i>
                    </button>
                )}
                <div>
                    <strong>{props.title}</strong>
                    {props.type && <small>{props.type}</small>}
                </div>
            </div>

            {props.navItems.length > 1 && (
                <div className="ttas-run-detail-nav">
                    <div className="ttas-run-nav-list">
                        {props.navItems.map(item => (
                            <button
                                key={`nav-${item.id}`}
                                type="button"
                                className={props.selectedSeq === item.seq ? 'is-selected' : ''}
                                title={timelineItemTitle(item, props.tr)}
                                onClick={(event) => {
                                    event.stopPropagation();
                                    props.onSelectNav(item);
                                }}
                            >
                                <i aria-hidden="true"></i>
                                <span>{timelineItemShortLabel(item, props.tr)}</span>
                            </button>
                        ))}
                    </div>
                </div>
            )}

            <div className="ttas-run-detail-scroll">
                {props.loading ? (
                    <div className="ttas-run-empty">
                        <i className="fa-solid fa-spinner fa-spin"></i>
                        <span>{props.tr('timelineLoadingDetails')}</span>
                    </div>
                ) : props.error ? (
                    <div className="ttas-run-detail-error">
                        <i className="fa-solid fa-triangle-exclamation"></i>
                        <span>{props.error}</span>
                    </div>
                ) : props.sections.length === 0 ? (
                    <div className="ttas-run-empty">
                        <i className="fa-solid fa-file-circle-question"></i>
                        <span>{props.tr('timelineDetailEmpty')}</span>
                    </div>
                ) : props.sections.map((section, index) => (
                    <article key={index} className="ttas-run-detail-section">
                        <div className="ttas-run-detail-section-head">
                            <strong>{props.tr(section.labelKey)}</strong>
                            {section.path && <small>{section.path}</small>}
                        </div>
                        {section.actions && section.actions.length > 0 && (
                            <div className="ttas-run-detail-actions">
                                {section.actions.map(action => (
                                    <button
                                        key={action.kind}
                                        type="button"
                                        className="menu_button ttas-run-detail-action"
                                        data-ttas-action={action.kind}
                                        title={props.tr(action.hintKey ?? action.labelKey)}
                                        onClick={(event) => {
                                            event.stopPropagation();
                                            props.onAction(action);
                                        }}
                                    >
                                        {action.icon && <i className={`fa-solid ${action.icon}`} aria-hidden="true"></i>}
                                        <span>{props.tr(action.labelKey)}</span>
                                    </button>
                                ))}
                            </div>
                        )}
                        {section.fields && section.fields.length > 0 && (
                            <div className="ttas-run-detail-fields">
                                {section.fields.map(field => (
                                    <span key={field.label}>
                                        <b>{field.label}</b>
                                        <em>{field.value}</em>
                                    </span>
                                ))}
                            </div>
                        )}
                        {section.blocks && section.blocks.length > 0 && (
                            <div className="ttas-run-detail-blocks">
                                {section.blocks.map((block, blockIndex) => (
                                    <details
                                        key={blockIndex}
                                        className={`ttas-run-detail-block kind-${block.kind ?? 'text'}`}
                                        data-ttas-block-kind={block.kind ?? 'text'}
                                        open={block.defaultOpen !== false}
                                    >
                                        <summary className="ttas-run-detail-block-head">
                                            <strong>{'labelKey' in block ? props.tr(block.labelKey) : block.label}</strong>
                                            <span className="ttas-run-detail-block-badges">
                                                {block.meta && <small>{block.meta}</small>}
                                                {block.kind !== 'diff' && block.truncated && (
                                                    <small>{props.tr('timelineTruncated')}</small>
                                                )}
                                                <i className="fa-solid fa-chevron-down" aria-hidden="true"></i>
                                            </span>
                                        </summary>
                                        {block.kind === 'diff' ? (
                                            <div className="ttas-run-diff" role="table">
                                                {block.rows.map((row, rowIndex) => (
                                                    <div
                                                        key={rowIndex}
                                                        className="ttas-run-diff-row"
                                                        data-ttas-diff-row={row.type}
                                                        role="row"
                                                    >
                                                        <span className="ttas-run-diff-gutter" role="cell">{row.oldLine || ''}</span>
                                                        <span className="ttas-run-diff-gutter" role="cell">{row.newLine || ''}</span>
                                                        <span className="ttas-run-diff-marker" role="cell">{row.marker}</span>
                                                        <code className="ttas-run-diff-code" role="cell">{row.text}</code>
                                                    </div>
                                                ))}
                                            </div>
                                        ) : <pre>{block.text}</pre>}
                                    </details>
                                ))}
                            </div>
                        )}
                    </article>
                ))}
            </div>
        </section>
    );
}

export function SubAgentTray(props: {
    tr: AgentSystemTr;
    expanded: boolean;
    tasks: readonly SubAgentTask[];
    title: string;
    onToggle: () => void;
    onSelect: (task: SubAgentTask) => void;
}) {
    if (props.tasks.length === 0) return null;
    return (
        <aside className={`ttas-subagent-tray${props.expanded ? ' is-expanded' : ''}`}>
            <button
                type="button"
                className="ttas-subagent-tray-toggle"
                aria-expanded={props.expanded}
                title={props.tr(props.expanded ? 'timelineCollapseSubAgents' : 'timelineExpandSubAgents')}
                onClick={props.onToggle}
            >
                <span className="ttas-subagent-stack" aria-hidden="true">
                    {props.tasks.slice(0, 4).map(task => (
                        <i key={`dot-${task.taskId}`} style={subAgentTaskStyle(task) as CSSProperties}></i>
                    ))}
                </span>
                <strong>{props.title}</strong>
                <i className={`fa-solid ${props.expanded ? 'fa-chevron-down' : 'fa-chevron-up'}`} aria-hidden="true"></i>
            </button>
            {props.expanded && (
                <div className="ttas-subagent-list">
                    {props.tasks.map(task => (
                        <button
                            key={task.taskId}
                            type="button"
                            className="ttas-subagent-item"
                            data-ttas-status={subAgentTaskTone(task)}
                            style={subAgentTaskStyle(task) as CSSProperties}
                            onClick={() => props.onSelect(task)}
                        >
                            <span className="ttas-subagent-color" aria-hidden="true"></span>
                            <span className="ttas-subagent-copy">
                                <strong>{task.displayName}</strong>
                                <small>{subAgentStatusLabel(task.status, props.tr)}</small>
                            </span>
                            <span className="ttas-subagent-open">
                                <i className="fa-solid fa-up-right-from-square" aria-hidden="true"></i>
                                <span>{props.tr('timelineOpenSubAgent')}</span>
                            </span>
                        </button>
                    ))}
                </div>
            )}
        </aside>
    );
}
