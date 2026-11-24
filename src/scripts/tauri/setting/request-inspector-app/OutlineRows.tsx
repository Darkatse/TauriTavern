import { Fragment } from 'react';

import { JsonTree } from './JsonTree';
import type { Translate } from './RequestInspectorContract';
import { Highlight, TextBlock } from './TextBlock';
import { ToolSchema } from './ToolSchema';
import { formatCount, formatTokens } from './format';
import type { OutlineRow } from './outline';

const SIZE_BAR_WIDTH = 36;

/** What every row of one outline shares. */
export type RowContext = {
    needle: string;
    scale: number;
    expanded: ReadonlySet<string>;
    full: ReadonlySet<string>;
    onToggle: (id: string) => void;
    onShowFull: (id: string) => void;
    onCopy: (text: string) => void;
    /** Shows the part at a JSON Pointer in the payload view. */
    onReveal: ((path: string) => void) | null;
    tr: Translate;
};

export function Size({ value, scale }: { value: number; scale: number | null }) {
    return (
        <span className="tt-ri-size" title={`${formatCount(value)} tokens`}>
            {scale !== null && (
                <i aria-hidden="true" style={{ width: `${Math.max(2, Math.round(value / scale * SIZE_BAR_WIDTH))}px` }} />
            )}
            {formatTokens(value)}
        </span>
    );
}

export function Row({ row, speaker, messageNote, ctx }: {
    row: OutlineRow;
    /** The wire role, on the first row of a message without a line of its own. */
    speaker: string;
    /** What a message says about itself, on its only row. */
    messageNote: string;
    ctx: RowContext;
}) {
    const open = row.body !== null && ctx.expanded.has(row.id);
    const note = [messageNote, row.note].filter(Boolean).join(' · ');
    const { tags } = row;
    const content = (
        <>
            <span className="tt-ri-role">{speaker}</span>
            {(row.name || note) && (
                <span className="tt-ri-name">
                    <Highlight text={row.name} needle={ctx.needle} />
                    {note && <small>{row.name ? ' · ' : ''}<Highlight text={note} needle={ctx.needle} /></small>}
                </span>
            )}
            {!open && (
                <span className={`tt-ri-lead${row.code ? ' tt-ri-code' : ''}`}>
                    <Highlight text={row.lead} needle={ctx.needle} />
                </span>
            )}
            {tags.map(tag => <span key={tag} className="tt-ri-tag">{tag}</span>)}
            {row.tokens !== null && <Size value={row.tokens} scale={ctx.scale} />}
        </>
    );

    if (row.body === null) {
        return <div className="tt-ri-row">{content}</div>;
    }
    // An open row with nothing but its role would be an empty line above its
    // text, so its button turns into a gutter beside the text. It stays the
    // same element in both states, which keeps the keyboard focus on it.
    const gutter = open && !row.name && !note && tags.length === 0 && row.tokens === null;
    return (
        <div className={gutter ? 'tt-ri-open' : undefined}>
            <button
                type="button"
                className={`tt-ri-row${gutter ? ' tt-ri-gutter' : ''}`}
                aria-expanded={open}
                {...(gutter ? { 'aria-label': ctx.tr('Collapse') } : {})}
                onClick={() => ctx.onToggle(row.id)}
            >
                {gutter ? <span className="tt-ri-role">{speaker || '▾'}</span> : content}
            </button>
            {open && <div className="tt-ri-body"><Body row={row} ctx={ctx} /></div>}
        </div>
    );
}

function Body({ row, ctx }: { row: OutlineRow; ctx: RowContext }) {
    const { body } = row;
    const { needle, tr, onCopy } = ctx;
    const text = (value: string, labels = body?.kind === 'text' ? body.labels : []) => (
        <TextBlock
            text={value}
            labels={labels}
            needle={needle}
            full={ctx.full.has(row.id)}
            onShowFull={() => ctx.onShowFull(row.id)}
            tr={tr}
        />
    );
    const actions = (copied: string) => (
        <div className="tt-ri-actions">
            <button type="button" className="tt-ri-link" onClick={() => onCopy(copied)}>{tr('Copy')}</button>
            {row.path !== null && ctx.onReveal && (
                <button type="button" className="tt-ri-link" onClick={() => ctx.onReveal?.(row.path ?? '')}>
                    {tr('Show in payload')}
                </button>
            )}
        </div>
    );

    switch (body?.kind) {
        case 'text':
            return <>{text(body.text)}{actions(body.text)}</>;
        case 'json':
            return (
                <>
                    <JsonTree value={body.value} label={row.name || row.lead} needle={needle} tr={tr} onCopy={onCopy} />
                    {actions(JSON.stringify(body.value, null, 2) ?? '')}
                </>
            );
        case 'table':
            return (
                <dl className="tt-ri-table">
                    {body.entries.map(({ key, value }) => (
                        <Fragment key={key}>
                            <dt className="tt-ri-key">{key}</dt>
                            <dd>
                                {value === null || typeof value !== 'object'
                                    ? <span className="tt-ri-literal">{String(value)}</span>
                                    : <JsonTree value={value} label={key} needle={needle} tr={tr} onCopy={onCopy} />}
                            </dd>
                        </Fragment>
                    ))}
                </dl>
            );
        case 'tool':
            return (
                <>
                    {body.description && text(body.description, [])}
                    <ToolSchema schema={body.schema} needle={needle} tr={tr} onCopy={onCopy} />
                    {actions(body.description)}
                </>
            );
        case 'rows':
            return body.rows.map(child => <Row key={child.id} row={child} speaker="" messageNote="" ctx={ctx} />);
        default:
            return null;
    }
}
