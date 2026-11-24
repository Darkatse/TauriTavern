import { useMemo, useState } from 'react';

import { OutlineView } from './OutlineView';
import { MatchCount, PayloadView, type RawPayload, type Reveal } from './PayloadView';
import type { LogView, RequestPreview, TextViewerOptions, Translate } from './RequestInspectorContract';
import { IconButton, type Mode } from './Toolbar';
import { fill, formatCount } from './format';
import { buildOutline, defaultOpenRows, openableRows, type Outline } from './outline';
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

export function SubjectView({ subject, raw, mode, reveal, onReveal, needle, onCopy, onViewText, tr }: {
    subject: Subject;
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
    const found = useMemo(() => needle ? filterOutline(outline, needle) : null, [outline, needle]);
    const summaries = useMemo(() => payloadSummaries(outline), [outline]);
    const openable = useMemo(() => openableRows(outline), [outline]);
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

    return (
        <>
            <div className="tt-ri-facts">
                <span>{facts(subject, outline, tr)}</span>
                {mode === 'structure' && (
                    <IconButton
                        icon={allOpen ? 'fa-angles-up' : 'fa-angles-down'}
                        label={allOpen ? tr('Collapse all') : tr('Expand all')}
                        onClick={() => setExpanded(allOpen ? new Set() : openable)}
                    />
                )}
            </div>
            {error !== null && <p className="tt-ri-error">{error}</p>}
            {preview && <Notices preview={preview} outline={outline} tr={tr} />}
            {mode === 'structure' ? (
                <>
                    {found && <MatchCount count={found.count} tr={tr} />}
                    <OutlineView
                        outline={found?.outline ?? outline}
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
            ) : (
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
            )}
        </>
    );
}
