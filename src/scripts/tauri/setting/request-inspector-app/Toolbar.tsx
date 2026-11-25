import { useEffect, useRef, type ReactNode } from 'react';

import type { LogIndexEntry, Selection, Translate } from './RequestInspectorContract';
import type { BaseOption, BaseRef } from './compare-base';
import { fill, logLabel } from './format';

export type Mode = 'structure' | 'payload' | 'compare';

/** The row that opens under the bar; one at a time. */
export type Panel = 'search' | 'more' | null;

export function IconButton({ icon, label, expanded, onClick }: {
    icon: string;
    label: string;
    expanded?: boolean;
    onClick: () => void;
}) {
    return (
        <button
            type="button"
            className="menu_button tt-ri-icon"
            title={label}
            aria-label={label}
            {...(expanded === undefined ? {} : { 'aria-expanded': expanded })}
            onClick={onClick}
        >
            <i className={`fa-solid ${icon}`} aria-hidden="true" />
        </button>
    );
}

function SearchRow({ query, onQuery, tr }: { query: string; onQuery: (query: string) => void; tr: Translate }) {
    const inputRef = useRef<HTMLInputElement>(null);
    useEffect(() => inputRef.current?.focus(), []);
    return (
        <input
            ref={inputRef}
            type="search"
            className="text_pole tt-ri-search"
            value={query}
            placeholder={tr('Search')}
            aria-label={tr('Search')}
            onChange={event => onQuery(event.target.value)}
        />
    );
}

/** Which request, which view, what it is compared with, and the two rows that open under the bar. */
export function Toolbar({ entries, selection, onSelect, mode, onMode, bases, base, onBase, panel, onPanel, query, onQuery, more, tr }: {
    entries: LogIndexEntry[];
    selection: Selection;
    onSelect: (selection: Selection) => void;
    mode: Mode;
    onMode: (mode: Mode) => void;
    bases: BaseOption[];
    base: string | null;
    onBase: (base: BaseRef) => void;
    panel: Panel;
    onPanel: (panel: Panel) => void;
    query: string;
    onQuery: (query: string) => void;
    more: ReactNode;
    tr: Translate;
}) {
    const logId = selection.kind === 'log' ? selection.id : null;
    const modes: [Mode, string][] = [['structure', tr('Structure')], ['payload', 'Payload'], ['compare', tr('Compare')]];

    return (
        <header className="tt-ri-top">
            <div className="tt-ri-bar">
                <div className="tt-ri-segments" role="group" aria-label={tr('Request')}>
                    <button
                        type="button"
                        className="tt-ri-segment"
                        aria-pressed={selection.kind === 'preview'}
                        onClick={() => onSelect({ kind: 'preview' })}
                    >
                        {tr('Preview request')}
                    </button>
                    {/* The placeholder is the value while the preview shows, so picking the same log again still changes it. */}
                    <select
                        className="tt-ri-segment tt-ri-logs"
                        aria-label={tr('Sent requests')}
                        data-active={selection.kind === 'log'}
                        value={logId ?? ''}
                        disabled={entries.length === 0}
                        onChange={event => onSelect({ kind: 'log', id: Number(event.target.value) })}
                    >
                        <option value="" disabled hidden>
                            {entries.length === 0 ? tr('Nothing sent yet') : fill(tr('Sent ({count})'), { count: entries.length })}
                        </option>
                        {[...entries].reverse().map(entry => (
                            <option key={entry.id} value={entry.id}>{logLabel(entry)}</option>
                        ))}
                    </select>
                </div>
                <div className="tt-ri-view">
                    <div className="tt-ri-segments" role="group" aria-label={tr('View')}>
                        {modes.map(([value, label]) => (
                            <button
                                key={value}
                                type="button"
                                className="tt-ri-segment"
                                aria-pressed={mode === value}
                                onClick={() => onMode(value)}
                            >
                                {label}
                            </button>
                        ))}
                    </div>
                    <IconButton
                        icon="fa-magnifying-glass"
                        label={tr('Search')}
                        expanded={panel === 'search'}
                        onClick={() => onPanel(panel === 'search' ? null : 'search')}
                    />
                    <IconButton
                        icon="fa-ellipsis"
                        label={tr('More')}
                        expanded={panel === 'more'}
                        onClick={() => onPanel(panel === 'more' ? null : 'more')}
                    />
                </div>
            </div>
            {mode === 'compare' && bases.length > 0 && (
                <label className="tt-ri-base">
                    <span>{tr('Compared with')}</span>
                    <select
                        className="text_pole tt-ri-base-select"
                        value={base ?? ''}
                        onChange={(event) => {
                            const picked = bases.find(option => option.key === event.target.value);
                            if (picked) {
                                onBase(picked.ref);
                            }
                        }}
                    >
                        {bases.map(option => <option key={option.key} value={option.key}>{option.label}</option>)}
                    </select>
                </label>
            )}
            {panel === 'search' && <SearchRow query={query} onQuery={onQuery} tr={tr} />}
            {panel === 'more' && more}
        </header>
    );
}

/** Copies of the shown request, and how many sent requests are kept. */
export function MorePanel({ copies, hint, keepInput, onKeepInput, applyingKeep, onApplyKeep, tr }: {
    copies: { label: string; run: () => void }[];
    hint: string;
    keepInput: string;
    onKeepInput: (value: string) => void;
    applyingKeep: boolean;
    onApplyKeep: () => void;
    tr: Translate;
}) {
    return (
        <div className="tt-ri-more">
            {copies.length > 0 && (
                <div className="tt-ri-more-row">
                    {copies.map(copy => (
                        <button key={copy.label} type="button" className="menu_button" onClick={copy.run}>
                            {copy.label}
                        </button>
                    ))}
                </div>
            )}
            {hint && <p className="tt-ri-dim">{hint}</p>}
            <div className="tt-ri-more-row">
                <span>{tr('Sent requests kept')}</span>
                <input
                    className="text_pole tt-ri-keep"
                    type="number"
                    min="1"
                    step="1"
                    value={keepInput}
                    aria-label={tr('Sent requests kept')}
                    onChange={event => onKeepInput(event.target.value)}
                />
                <button type="button" className="menu_button" disabled={applyingKeep} onClick={onApplyKeep}>
                    {tr('Apply')}
                </button>
            </div>
        </div>
    );
}
