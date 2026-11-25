import { type ReactNode, useMemo, useState } from 'react';

import { OutlineView } from './OutlineView';
import { MatchCount, PayloadView, type RawPayload, type Reveal } from './PayloadView';
import type { LogView, RequestPreview, TextViewerOptions, Translate } from './RequestInspectorContract';
import { IconButton, type Mode } from './Toolbar';
import { type Comparable, type Comparison, compareRequests } from './compare';
import { fill, formatCount } from './format';
import { buildOutline, changedRows, defaultOpenRows, openableRows, type Outline } from './outline';
import { filterOutline } from './outline-search';
import { payloadSummaries } from './payload-summaries';
import type { LoadState } from './useLoad';

/** What the inspector shows: the next request, or one that was sent. */
export type Subject =
    | { kind: 'preview'; preview: RequestPreview }
    | { kind: 'log'; log: LogView };

function facts(subject: Subject, outline: Outline, tr: Translate): string {
    const count = (key: string, value: number) => fill(tr(key), { count: formatCount(value) });
    if (subject.kind === 'preview') {
        const { preview } = subject;
        return [
            preview.source,
            preview.model,
            outline.tokens === null ? '' : count('about {count} tokens', outline.tokens),
            count('{count} messages', outline.messageCount),
        ].filter(Boolean).join(' · ');
    }
    const { log } = subject;
    const usage = log.view.response?.usage;
    const paired = (key: string, value: number, pairedKey: string, extra: number | undefined) => extra === undefined
        ? count(key, value)
        : fill(tr(pairedKey), { count: formatCount(value), extra: formatCount(extra) });
    return [
        log.source,
        log.stream ? tr('Streaming') : '',
        usage?.input === undefined ? '' : paired('Input {count}', usage.input, 'Input {count} ({extra} cached)', usage.cacheRead),
        usage?.output === undefined ? '' : paired('Output {count}', usage.output, 'Output {count} ({extra} reasoning)', usage.reasoning),
    ].filter(Boolean).join(' · ');
}

function Differences({ comparison, tr }: { comparison: Comparison; tr: Translate }) {
    const { added, removed, changed, same, tokens } = comparison;
    if (added + removed + changed === 0) {
        return <span>{tr('Nothing differs from the base')}</span>;
    }
    const counted = (count: number, sign: string, change: string, label: string) => count > 0 && (
        <span><span className="tt-ri-counts"><span data-change={change}>{sign}{formatCount(count)}</span></span> {tr(label)}</span>
    );
    return (
        <span className="tt-ri-differences">
            {counted(added, '+', 'added', 'Added')}
            {counted(removed, '−', 'removed', 'Removed')}
            {counted(changed, '~', 'changed', 'Changed')}
            {same > 0 && <span>{fill(tr('{count} unchanged'), { count: formatCount(same) })}</span>}
            {tokens !== null && tokens !== 0 && (
                <span>{fill(tr('about {count} tokens'), { count: `${tokens > 0 ? '+' : '−'}${formatCount(Math.abs(tokens))}` })}</span>
            )}
        </span>
    );
}

/** Lines that appear only when they apply to this preview. */
function Notices({ preview, outline, tr }: { preview: RequestPreview; outline: Outline; tr: Translate }) {
    const listed = (title: string, items: string[]) => items.length > 0 && (
        <details className="tt-ri-notice">
            <summary>{fill(tr(title), { count: items.length })}</summary>
            <ul>{items.map((item, index) => <li key={index}>{item}</li>)}</ul>
        </details>
    );
    return (
        <>
            {preview.assemblyError !== null && (
                <p className="tt-ri-notice tt-ri-error">
                    {fill(tr('The prompt was assembled with an error: {error}'), { error: preview.assemblyError })}
                </p>
            )}
            {listed('{count} MCP tools cannot be used this time', preview.mcpDiagnostics.map(diagnostic => diagnostic.message))}
            {listed(
                '{count} sources could not be found in the request',
                outline.unlocated.map(source => `${source.name} · ${formatCount(source.tokens)} tokens`),
            )}
        </>
    );
}

export function SubjectView({ subject, base, raw, mode, reveal, onReveal, needle, onCopy, onViewText, tr }: {
    subject: Subject;
    base: LoadState<Comparable> | null;
    raw: LoadState<RawPayload>;
    mode: Mode;
    reveal: Reveal | null;
    onReveal: (reveal: Reveal) => void;
    needle: string;
    onCopy: (text: string) => void;
    onViewText: (options: TextViewerOptions) => void;
    tr: Translate;
}) {
    const preview = subject.kind === 'preview' ? subject.preview : null;
    const record = subject.kind === 'preview' ? subject.preview : subject.log;
    const { view } = record;
    const outline = useMemo(() => buildOutline(view, preview, tr), [view, preview, tr]);
    const comparison = useMemo(
        () => mode === 'compare' && base?.status === 'ready'
            ? compareRequests(base.value, { view, assembly: preview }, tr)
            : null,
        [mode, base, view, preview, tr],
    );
    const listed = comparison?.outline ?? outline;
    const found = useMemo(() => needle ? filterOutline(listed, needle) : null, [listed, needle]);
    const summaries = useMemo(() => payloadSummaries(outline), [outline]);
    const openable = useMemo(() => comparison ? changedRows(comparison.outline) : openableRows(outline), [comparison, outline]);
    const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => defaultOpenRows(outline));
    const [full, setFull] = useState<ReadonlySet<string>>(() => new Set());

    const allOpen = openable.size > 0 && [...openable].every(id => expanded.has(id));
    const failed = subject.kind === 'log' && !subject.log.ok;
    const error = view.response?.error ?? (failed ? tr('The request failed') : null);
    const toggle = (id: string) => setExpanded(previous => {
        const next = new Set(previous);
        if (!next.delete(id)) {
            next.add(id);
        }
        return next;
    });

    let content: ReactNode;
    if (mode === 'payload') {
        content = (
            <PayloadView
                http={view.request.http ?? null}
                endpoint={record.endpoint}
                raw={raw}
                summaries={summaries}
                reveal={reveal}
                needle={needle}
                tr={tr}
                onCopy={onCopy}
                onViewText={onViewText}
            />
        );
    } else if (mode === 'compare' && !comparison) {
        content = (
            <div className="tt-ri-empty">
                {base?.status === 'error'
                    ? <p className="tt-ri-error">{base.error}</p>
                    : <p>{base ? tr('Loading...') : tr('No other request to compare with')}</p>}
            </div>
        );
    } else {
        content = (
            <>
                {found && <MatchCount count={found.count} tr={tr} />}
                <OutlineView
                    outline={found?.outline ?? listed}
                    signed={comparison !== null}
                    needle={needle}
                    expanded={expanded}
                    full={full}
                    onToggle={toggle}
                    onShowFull={id => setFull(previous => new Set(previous).add(id))}
                    onCopy={onCopy}
                    onReveal={(pointer, response) => onReveal({ target: response ? 'response' : 'request', pointer })}
                    tr={tr}
                />
            </>
        );
    }

    return (
        <>
            <div className="tt-ri-facts">
                {comparison ? <Differences comparison={comparison} tr={tr} /> : <span>{facts(subject, outline, tr)}</span>}
                {(mode === 'structure' || comparison) && (
                    <IconButton
                        icon={allOpen ? 'fa-angles-up' : 'fa-angles-down'}
                        label={allOpen ? tr('Collapse all') : tr('Expand all')}
                        onClick={() => setExpanded(allOpen ? new Set() : openable)}
                    />
                )}
            </div>
            {error !== null && <p className="tt-ri-error">{error}</p>}
            {preview && <Notices preview={preview} outline={outline} tr={tr} />}
            {content}
        </>
    );
}
