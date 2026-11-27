import type { ReactNode } from 'react';

import type { McpTranslator } from './host';

export function OutcomeView({ outcome, tr }: { outcome: TauriTavernMcpTestCallOutcome; tr: McpTranslator }): ReactNode {
    if (outcome.outcome === 'not_sent') {
        return (
            <>
                <h4 role="status"><i className="fa-solid fa-ban" aria-hidden="true" />{tr('notSent')}</h4>
                <p>{outcome.message}</p>
                <code className="tt-mcp-outcome-code">{outcome.code}</code>
            </>
        );
    }
    if (outcome.outcome === 'outcome_unknown') {
        return (
            <>
                <h4 className="is-danger" role="status"><i className="fa-solid fa-circle-question" aria-hidden="true" />{tr('outcomeUnknown')}</h4>
                <p>{outcome.message}</p>
                <p className="tt-mcp-note">{tr('outcomeUnknownHint')}</p>
                <code className="tt-mcp-outcome-code">{outcome.code}</code>
            </>
        );
    }

    const response = outcome.response;
    if (response.kind === 'server_error') {
        return (
            <>
                <h4 className="is-danger" role="status"><i className="fa-solid fa-circle-exclamation" aria-hidden="true" />{tr('serverError')}</h4>
                <p>{response.message}</p>
                <code className="tt-mcp-outcome-code">{response.code}</code>
                {response.dataJson !== undefined && <pre>{response.dataJson}</pre>}
            </>
        );
    }
    if (response.kind === 'rejected') {
        return (
            <>
                <h4 className="is-danger" role="status"><i className="fa-solid fa-hand" aria-hidden="true" />{tr('rejected')}</h4>
                <p>{tr('rejectedHint')}</p>
                <code className="tt-mcp-outcome-code">
                    {response.retryAfter === undefined
                        ? `HTTP ${response.httpStatus}`
                        : `HTTP ${response.httpStatus} · Retry-After: ${response.retryAfter}`}
                </code>
            </>
        );
    }
    if (response.kind === 'unsupported_response') {
        return (
            <>
                <h4 role="status"><i className="fa-solid fa-circle-info" aria-hidden="true" />{tr('unsupportedResponse')}</h4>
                <p>{response.message}</p>
                <code className="tt-mcp-outcome-code">{response.responseType}</code>
            </>
        );
    }

    return (
        <>
            <h4 className={response.isError ? 'is-danger' : 'is-ok'} role="status">
                <i className={`fa-solid ${response.isError ? 'fa-circle-exclamation' : 'fa-circle-check'}`} aria-hidden="true" />
                {tr(response.isError ? 'toolError' : 'serverResponded')}
            </h4>
            {response.textBlocks.map(block => <pre key={block.index}>{block.text}</pre>)}
            {response.structuredJson !== undefined && (
                <>
                    <b>{tr('structuredResult')}</b>
                    <pre className="tt-mcp-json-output">{response.structuredJson}</pre>
                </>
            )}
            {response.diagnostics.length > 0 && (
                <details className="tt-mcp-details">
                    <summary>{tr('diagnostics')} · {response.diagnostics.length}</summary>
                    <ul>
                        {response.diagnostics.map(diagnostic => (
                            <li key={`${diagnostic.code}:${diagnostic.contentIndex ?? ''}:${diagnostic.message}`}>
                                <code>
                                    {diagnostic.contentIndex === undefined
                                        ? diagnostic.code
                                        : `${diagnostic.code} [${diagnostic.contentIndex}]`}
                                </code>
                                <span>{diagnostic.message}</span>
                            </li>
                        ))}
                    </ul>
                </details>
            )}
            {response.textBlocks.length === 0 && response.structuredJson === undefined && (
                <p className="tt-mcp-note">{tr('noDisplayableContent')}</p>
            )}
        </>
    );
}
