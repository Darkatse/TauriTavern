import { Fragment, useMemo, useState } from 'react';

import { JsonTree } from './JsonTree';
import type { RequestDescription, TextViewerOptions, Translate } from './RequestInspectorContract';
import { fill, formatCount } from './format';
import { searchTree } from './json-tree';
import type { PayloadSummaries } from './payload-summaries';
import type { LoadState } from './useLoad';

/** The request body and the recorded response as stored; a preview has no response. */
export type RawPayload = {
    requestRaw: string;
    responseRaw: string | null;
    responseRawKind: 'json' | 'sse' | null;
};

/** A node to bring into view, by JSON Pointer into one of the two bodies. */
export type Reveal = { target: 'request' | 'response'; pointer: string };

function parseJson(text: string): unknown {
    try {
        return JSON.parse(text) as unknown;
    } catch {
        return text;
    }
}

/** A recorded stream holds one event per line. */
function parseEvents(text: string): unknown[] {
    return text.split('\n').filter(line => line.trim()).map(parseJson);
}

export function MatchCount({ count, tr }: { count: number; tr: Translate }) {
    return (
        <p className="tt-ri-dim" role="status">
            {count > 0 ? fill(tr('{count} matches'), { count: formatCount(count) }) : tr('No matches')}
        </p>
    );
}

function RequestLine({ http, endpoint, tr }: { http: RequestDescription | null; endpoint: string; tr: Translate }) {
    const [open, setOpen] = useState(false);
    if (!http) {
        return <div className="tt-ri-http"><span className="tt-ri-url">{endpoint}</span></div>;
    }
    return (
        <>
            <div className="tt-ri-http">
                <strong>{http.transport === 'websocket' ? tr('WebSocket handshake') : http.method}</strong>
                <span className="tt-ri-url">{http.url}</span>
                <button type="button" className="tt-ri-link" aria-expanded={open} onClick={() => setOpen(!open)}>
                    {fill(tr('Headers {count}'), { count: http.headers.length })}
                </button>
            </div>
            {open && (
                <>
                    <dl className="tt-ri-table">
                        {http.headers.map(({ name, value }, index) => (
                            <Fragment key={index}>
                                <dt className="tt-ri-key">{name}</dt>
                                <dd>{value}</dd>
                            </Fragment>
                        ))}
                    </dl>
                    <p className="tt-ri-dim">
                        {tr('Headers the HTTP client adds on its own, such as Host and Content-Length, are not listed.')}
                    </p>
                </>
            )}
        </>
    );
}

/** The wire request as it is: request line, headers, and the bodies as trees. */
export function PayloadView({ http, endpoint, raw, summaries, reveal, needle, tr, onCopy, onViewText }: {
    http: RequestDescription | null;
    endpoint: string;
    raw: LoadState<RawPayload>;
    summaries: PayloadSummaries;
    reveal: Reveal | null;
    needle: string;
    tr: Translate;
    onCopy: (text: string) => void;
    onViewText: (options: TextViewerOptions) => void;
}) {
    const payload = raw.status === 'ready' ? raw.value : null;
    const request = useMemo(() => payload && parseJson(payload.requestRaw), [payload]);
    const response = useMemo(() => {
        if (payload?.responseRaw == null) {
            return undefined;
        }
        return payload.responseRawKind === 'sse' ? parseEvents(payload.responseRaw) : parseJson(payload.responseRaw);
    }, [payload]);
    const found = useMemo(() => needle && payload ? {
        request: searchTree(request, needle),
        response: response === undefined ? null : searchTree(response, needle),
    } : null, [needle, payload, request, response]);

    const section = (title: string, note: string, text: string) => (
        <h4 className="tt-ri-divider">
            {title}
            {note && <small>{note}</small>}
            <button type="button" className="tt-ri-link" onClick={() => onViewText({ title, text, wrap: 'off' })}>
                {tr('View raw JSON')}
            </button>
        </h4>
    );

    return (
        <>
            <RequestLine http={http} endpoint={endpoint} tr={tr} />
            {raw.status === 'loading' && <p className="tt-ri-dim">{tr('Loading...')}</p>}
            {raw.status === 'error' && <p className="tt-ri-error">{raw.error}</p>}
            {payload && (
                <>
                    {found && <MatchCount count={found.request.count + (found.response?.count ?? 0)} tr={tr} />}
                    {section(tr('Request body'), '', payload.requestRaw)}
                    <JsonTree
                        value={request}
                        label={tr('Request body')}
                        summaries={summaries.request}
                        open={summaries.open}
                        reveal={reveal?.target === 'request' ? reveal.pointer : null}
                        needle={needle}
                        keep={found?.request.keep ?? null}
                        tr={tr}
                        onCopy={onCopy}
                    />
                    {payload.responseRaw !== null && response !== undefined && (
                        <>
                            {section(
                                tr('Response body'),
                                Array.isArray(response) && payload.responseRawKind === 'sse'
                                    ? fill(tr('Event stream · {count} events'), { count: formatCount(response.length) })
                                    : '',
                                payload.responseRaw,
                            )}
                            <JsonTree
                                value={response}
                                label={tr('Response body')}
                                summaries={summaries.response}
                                reveal={reveal?.target === 'response' ? reveal.pointer : null}
                                needle={needle}
                                keep={found?.response?.keep ?? null}
                                tr={tr}
                                onCopy={onCopy}
                            />
                        </>
                    )}
                </>
            )}
        </>
    );
}
