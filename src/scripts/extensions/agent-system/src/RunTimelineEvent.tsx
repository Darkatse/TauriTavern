import type { CSSProperties } from 'react';

import type { AgentSystemTr } from './i18n';
import type { TimelineItem, TimelineLiveContent } from './RunTimelineContract';
import { timelineItemShortLabel, timelineItemTime, timelineItemTitle } from './run-timeline-display';
import { timelineItemHeightPx, timelineItemRowSpan } from './run-timeline-virtual-list';

type TimelineEventProps = {
    item: TimelineItem;
    tr: AgentSystemTr;
    selected: boolean;
    latest: boolean;
    active: boolean;
    onActivate: () => void;
};

export function RunTimelineEvent({ item, tr, selected, latest, active, onActivate }: TimelineEventProps) {
    const live = item.live;
    const time = timelineItemTime(item);
    const style = {
        '--ttas-run-event-item-height': `${timelineItemHeightPx(item)}px`,
    } as CSSProperties;
    const header = <>
        <span className="ttas-run-event-icon" aria-hidden="true">
            <i className={`fa-solid ${item.icon}`}></i>
        </span>
        <span className="ttas-run-event-copy">
            <span className="ttas-run-event-title">
                {live?.streamTone === 'reasoning' && live.toolLabel && (
                    <span className="ttas-run-event-tool-names" title={live.toolLabel}>{live.toolLabel}</span>
                )}
                <span className="ttas-run-event-label">{timelineItemTitle(item, tr)}</span>
                {live && <TimelineLiveMetric live={live} tr={tr} />}
                {active && (
                    <span className="ttas-run-ellipsis" aria-hidden="true"><i>.</i><i>.</i><i>.</i></span>
                )}
                {live && <i className="fa-solid fa-chevron-right ttas-run-event-chevron" aria-hidden="true"></i>}
            </span>
            {!live && item.summary && <small>{item.summary}</small>}
        </span>
        <span className="ttas-run-event-meta">
            <em>{timelineItemShortLabel(item, tr)}</em>
            {time && <time>{time}</time>}
        </span>
    </>;

    return (
        <li
            className={[
                'ttas-run-event', `tone-${item.tone}`, `kind-${item.kind}`,
                latest && 'is-latest', active && 'is-active', selected && 'is-selected', live && 'is-live',
            ].filter(Boolean).join(' ')}
            data-ttas-kind={item.kind}
            data-ttas-row-span={timelineItemRowSpan(item)}
            aria-live={live ? 'off' : undefined}
            style={style}
        >
            {live ? (
                <details className="ttas-run-event-card" open={live.expanded}>
                    <summary
                        aria-expanded={live.expanded}
                        onClick={event => { event.preventDefault(); onActivate(); }}
                    >
                        {header}
                        {!live.expanded && (
                            <span className={`ttas-run-event-live is-streaming is-${live.streamTone}`} aria-hidden="true">
                                <span className="ttas-run-event-live-stream" data-ttas-truncated={live.truncated ? '' : undefined}>
                                    {live.tail}
                                </span>
                            </span>
                        )}
                    </summary>
                    {live.expanded && live.blocks.map((block, index) => block.text && (
                        <div className={`ttas-run-event-live is-${block.streamTone}${block.streamTone === live.streamTone ? ' is-streaming' : ''}`} key={index}>
                            {block.labelKey && <small className="ttas-run-event-live-label">{tr(block.labelKey)}</small>}
                            <span className="ttas-run-event-live-stream">{block.text}</span>
                        </div>
                    ))}
                </details>
            ) : (
                <button className="ttas-run-event-card" type="button" onClick={onActivate}>{header}</button>
            )}
        </li>
    );
}

function TimelineLiveMetric({ live, tr }: { live: TimelineLiveContent; tr: AgentSystemTr }) {
    if (live.streamTone === 'reasoning') return null;
    if (live.addedWords === 0 && live.removedWords === 0) return null;
    if (live.toolId === 'builtin:workspace.apply_patch') {
        return (
            <em className="ttas-run-event-live-metric">
                (<span className="is-added">+{tr('timelineWordCount', { count: live.addedWords })}</span>
                {' / '}
                <span className="is-removed">-{tr('timelineWordCount', { count: live.removedWords })}</span>)
            </em>
        );
    }
    return <em className="ttas-run-event-live-metric">+{tr('timelineWordCount', { count: live.addedWords })}</em>;
}
