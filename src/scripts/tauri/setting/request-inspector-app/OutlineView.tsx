import { useState } from 'react';

import { Row, Size, type RowContext } from './OutlineRows';
import { fill, formatCount } from './format';
import type { Outline, OutlineFold, OutlineMessage } from './outline';

/** Messages and folds drawn per step. */
const PAGE = 200;

/**
 * A message of several rows gets a line of its own when there is something to
 * say about the whole message; otherwise its first row carries the role.
 */
function MessageBlock({ message, ctx }: { message: OutlineMessage; ctx: RowContext }) {
    const { rows } = message;
    const titled = rows.length !== 1 && (rows.length === 0 || message.note !== '' || message.tokens !== null);
    return (
        <section className={`tt-ri-message tt-ri-tone-${message.tone}`}>
            {titled && (
                <div className="tt-ri-row tt-ri-row-head">
                    <span className="tt-ri-role">{message.role}</span>
                    <span className="tt-ri-lead">{message.note}</span>
                    {message.tokens !== null && <Size value={message.tokens} scale={null} />}
                </div>
            )}
            {rows.map((row, index) => (
                <Row
                    key={row.id}
                    row={row}
                    speaker={!titled && index === 0 ? message.role : ''}
                    messageNote={rows.length === 1 ? message.note : ''}
                    ctx={ctx}
                />
            ))}
        </section>
    );
}

function FoldBlock({ fold, ctx }: { fold: OutlineFold; ctx: RowContext }) {
    const open = ctx.expanded.has(fold.id);
    return (
        <>
            <section className="tt-ri-message tt-ri-fold">
                <button type="button" className="tt-ri-row" aria-expanded={open} onClick={() => ctx.onToggle(fold.id)}>
                    <span className="tt-ri-role" />
                    <span className="tt-ri-name">{fold.label}</span>
                    <span className="tt-ri-lead">{fold.note}</span>
                    {fold.tokens !== null && <Size value={fold.tokens} scale={ctx.scale} />}
                </button>
            </section>
            {open && fold.messages.map(message => <MessageBlock key={message.id} message={message} ctx={ctx} />)}
        </>
    );
}

/**
 * The request as an outline: one line per piece, opened on demand. `onReveal`
 * takes the pointer and whether it belongs to the response body.
 */
export function OutlineView({ outline, onReveal, ...shared }: Omit<RowContext, 'scale' | 'onReveal'> & {
    outline: Outline;
    onReveal: ((path: string, response: boolean) => void) | null;
}) {
    const [limit, setLimit] = useState(PAGE);
    const { tr } = shared;
    const ctx = (response: boolean): RowContext => ({
        ...shared,
        scale: outline.scale,
        onReveal: onReveal && (path => onReveal(path, response)),
    });
    const request = ctx(false);
    const rest = outline.items.length - limit;

    return (
        <div className="tt-ri-outline">
            {outline.head.map(row => (
                <section key={row.id} className="tt-ri-message">
                    <Row row={row} speaker="" messageNote="" ctx={request} />
                </section>
            ))}
            {outline.items.slice(0, limit).map(item => item.kind === 'fold'
                ? <FoldBlock key={item.fold.id} fold={item.fold} ctx={request} />
                : <MessageBlock key={item.message.id} message={item.message} ctx={request} />)}
            {rest > 0 && (
                <button type="button" className="tt-ri-link" onClick={() => setLimit(limit + PAGE)}>
                    {fill(tr('Show {count} more'), { count: formatCount(rest) })}
                </button>
            )}
            {outline.response && (
                <>
                    <h4 className="tt-ri-divider">
                        {tr('Model response')}
                        {outline.response.finishReason !== null && (
                            <small>{fill(tr('Finish reason: {reason}'), { reason: outline.response.finishReason })}</small>
                        )}
                    </h4>
                    {outline.response.messages.map(message => (
                        <MessageBlock key={message.id} message={message} ctx={ctx(true)} />
                    ))}
                </>
            )}
        </div>
    );
}
