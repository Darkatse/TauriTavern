import { Fragment } from 'react';

import { DiffBlock } from './DiffBlock';
import { JsonTree } from './JsonTree';
import type { Translate } from './RequestInspectorContract';
import { Highlight, TextBlock } from './TextBlock';
import { ToolSchema } from './ToolSchema';
import { formatCount, formatTokens, isScalar, prettyJson } from './format';
import type { OutlineRow, RowChange, TableEntry } from './outline';
import { diffLines } from './text-diff';

const SIZE_BAR_WIDTH = 36;

const SIGNS = {
    added: ['+', 'Added'],
    removed: ['−', 'Removed'],
    changed: ['~', 'Changed'],
} as const;

/** What every row of one outline shares. */
export type RowContext = {
    needle: string;
    /** Every row has a sign column, so the rows that differ line up with the ones that do not. */
    signed: boolean;
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

export function Sign({ change, tr }: { change: RowChange['kind'] | null; tr: Translate }) {
    if (change === null) {
        return <span className="tt-ri-sign" />;
    }
    const [sign, label] = SIGNS[change];
    return <span className="tt-ri-sign" data-change={change} role="img" aria-label={tr(label)}>{sign}</span>;
}

/** A search replaces the lead, so the mark of a change no longer points into it. */
function Lead({ row, needle }: { row: OutlineRow; needle: string }) {
    const mark = needle === '' && row.change?.kind === 'changed' ? row.change.mark : null;
    if (mark === null) {
        return <Highlight text={row.lead} needle={needle} />;
    }
    const Changed = mark.op === 'removed' ? 'del' : 'ins';
    return (
        <>
            {row.lead.slice(0, mark.start)}
            <Changed>{row.lead.slice(mark.start, mark.end)}</Changed>
            {row.lead.slice(mark.end)}
        </>
    );
}

function LineCounts({ added, removed }: { added: number; removed: number }) {
    return (
        <span className="tt-ri-counts">
            {added > 0 && <span data-change="added">+{formatCount(added)}</span>}
            {removed > 0 && <span data-change="removed">−{formatCount(removed)}</span>}
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
    const { tags, change } = row;
    const counts = change?.kind === 'changed' && change.added + change.removed > 0 ? change : null;
    const content = (
        <>
            {ctx.signed && <Sign change={change?.kind ?? null} tr={ctx.tr} />}
            <span className="tt-ri-role">{speaker}</span>
            {(row.name || note) && (
                <span className="tt-ri-name">
                    <Highlight text={row.name} needle={ctx.needle} />
                    {note && <small>{row.name ? ' · ' : ''}<Highlight text={note} needle={ctx.needle} /></small>}
                </span>
            )}
            {!open && (
                <span className={`tt-ri-lead${row.code ? ' tt-ri-code' : ''}`}>
                    <Lead row={row} needle={ctx.needle} />
                </span>
            )}
            {tags.map(tag => <span key={tag} className="tt-ri-tag">{tag}</span>)}
            {counts && <LineCounts added={counts.added} removed={counts.removed} />}
            {!counts && row.tokens !== null && <Size value={row.tokens} scale={ctx.scale} />}
        </>
    );

    if (row.body === null) {
        return <div className="tt-ri-row" data-change={change?.kind}>{content}</div>;
    }
    // An open row with nothing but its role would be an empty line above its
    // text, so its button turns into a gutter beside the text. It stays the
    // same element in both states, which keeps the keyboard focus on it.
    const gutter = open && !ctx.signed && !row.name && !note && tags.length === 0 && row.tokens === null;
    return (
        <div className={gutter ? 'tt-ri-open' : undefined}>
            <button
                type="button"
                className={`tt-ri-row${gutter ? ' tt-ri-gutter' : ''}`}
                data-change={change?.kind}
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

function TableValue({ entry, row, ctx }: { entry: TableEntry; row: OutlineRow; ctx: RowContext }) {
    const { key, value, change } = entry;
    const { needle, tr, onCopy } = ctx;
    if (change?.kind === 'changed' && !(isScalar(value) && isScalar(change.before))) {
        return (
            <DiffBlock
                lines={diffLines(prettyJson(change.before), prettyJson(value))}
                needle={needle}
                full={ctx.full.has(row.id)}
                onShowFull={() => ctx.onShowFull(row.id)}
                tr={tr}
            />
        );
    }
    const shown = isScalar(value)
        ? <span className="tt-ri-literal">{String(value)}</span>
        : <JsonTree value={value} label={key} needle={needle} tr={tr} onCopy={onCopy} />;
    switch (change?.kind) {
        case 'changed': return <><del>{String(change.before)}</del> → <ins>{shown}</ins></>;
        case 'added': return <ins>{shown}</ins>;
        case 'removed': return <del>{shown}</del>;
        default: return shown;
    }
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
                    {actions(prettyJson(body.value))}
                </>
            );
        case 'table':
            return (
                <dl className="tt-ri-table">
                    {body.entries.map(entry => (
                        <Fragment key={entry.key}>
                            <dt className="tt-ri-key" data-change={entry.change?.kind}>
                                {entry.change ? `${SIGNS[entry.change.kind][0]} ` : ''}{entry.key}
                            </dt>
                            <dd><TableValue entry={entry} row={row} ctx={ctx} /></dd>
                        </Fragment>
                    ))}
                </dl>
            );
        case 'diff':
            return (
                <>
                    <DiffBlock
                        lines={body.lines}
                        needle={needle}
                        full={ctx.full.has(row.id)}
                        onShowFull={() => ctx.onShowFull(row.id)}
                        tr={tr}
                    />
                    {actions(body.lines.flatMap(line => line.op === 'removed' ? [] : [line.text]).join('\n'))}
                </>
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
