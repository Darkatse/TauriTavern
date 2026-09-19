import { useLayoutEffect, useRef, useState } from 'react';

import type { DevLogsActions, DevLogsTranslate, LiveLogClient, LiveLogEntry } from './DevLogsContract';
import { DevLogButton, DevLogToggle, LogRow } from './DevLogComponents';
import { useAsyncSubscription } from './useAsyncSubscription';
import {
    LIVE_LOG_PANEL_BUFFER_LIMIT,
    LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE,
    LIVE_LOG_PANEL_MAX_WINDOW_SIZE,
    LIVE_LOG_PANEL_WINDOW_GROW_STEP,
    LOG_LEVEL_OPTIONS,
    entryMatchesLevel,
    formatEntryLine,
    levelClass,
} from './log-utils';

const NEAR_BOTTOM_THRESHOLD_PX = 24;

function isNearBottom(container: HTMLElement | null): boolean {
    if (!container) {
        return true;
    }
    return container.scrollHeight - container.scrollTop - container.clientHeight < NEAR_BOTTOM_THRESHOLD_PX;
}

function trimEntries(entries: LiveLogEntry[], trimEntriesInPlace: ((entries: LiveLogEntry[]) => void) | null): void {
    if (trimEntriesInPlace) {
        trimEntriesInPlace(entries);
        return;
    }
    if (entries.length > LIVE_LOG_PANEL_BUFFER_LIMIT) {
        entries.splice(0, entries.length - LIVE_LOG_PANEL_BUFFER_LIMIT);
    }
}

function countMatching(entries: LiveLogEntry[], filter: string): number {
    return entries.reduce((count, entry) => count + (entryMatchesLevel(entry, filter) ? 1 : 0), 0);
}

export type LiveLogPanelProps = {
    title: string;
    initialEntries?: LiveLogEntry[];
    client: LiveLogClient;
    actions: DevLogsActions;
    tr: DevLogsTranslate;
    showConsoleCapture?: boolean;
    consoleCaptureEnabled?: boolean;
    trimEntriesInPlace?: ((entries: LiveLogEntry[]) => void) | null;
};

export function LiveLogPanel({
    title,
    initialEntries = [],
    client,
    actions,
    tr,
    showConsoleCapture = false,
    consoleCaptureEnabled = false,
    trimEntriesInPlace = null,
}: LiveLogPanelProps) {
    // The canonical buffer is push-heavy but only the projected tail renders,
    // so it lives in a stable box instead of reactive state.
    const [buffer] = useState(() => {
        const entries = initialEntries.slice();
        trimEntries(entries, trimEntriesInPlace);
        return { entries };
    });
    const [filter, setFilter] = useState('ALL');
    const [windowSize, setWindowSize] = useState(LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE);
    const [paused, setPaused] = useState(false);
    const [consoleCapture, setConsoleCapture] = useState(consoleCaptureEnabled);
    const [rendered, setRendered] = useState<LiveLogEntry[]>(
        () => projectTail(buffer.entries, 'ALL', LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE),
    );
    // Status line facts: the filtered buffer count at projection time, plus
    // the would-be-visible entries that arrived since without being projected.
    const [totalShown, setTotalShown] = useState(() => countMatching(buffer.entries, 'ALL'));
    const [newCount, setNewCount] = useState(0);

    const listRef = useRef<HTMLDivElement | null>(null);
    // Scroll intent for the next rendered commit; consumed by useLayoutEffect.
    const followTailRef = useRef(true);
    const wasNearBottomRef = useRef(true);

    useLayoutEffect(() => {
        if (!followTailRef.current) {
            return;
        }
        followTailRef.current = false;
        const list = listRef.current;
        if (list) {
            list.scrollTop = list.scrollHeight;
        }
    }, [rendered]);

    function trimBuffer(): void {
        trimEntries(buffer.entries, trimEntriesInPlace);
    }

    function projectTail(entries: LiveLogEntry[], activeFilter: string, activeWindow: number): LiveLogEntry[] {
        return entries
            .filter(entry => entryMatchesLevel(entry, activeFilter))
            .slice(-Math.min(activeWindow, LIVE_LOG_PANEL_MAX_WINDOW_SIZE));
    }

    function commitTail(activeFilter: string, activeWindow: number): void {
        wasNearBottomRef.current = true;
        followTailRef.current = true;
        setTotalShown(countMatching(buffer.entries, activeFilter));
        setNewCount(0);
        setRendered(projectTail(buffer.entries, activeFilter, activeWindow));
    }

    // Rebuild the projected tail from the buffer and land on it.
    function materializeTail(overrides?: { filter?: string; windowSize?: number }): void {
        const activeFilter = overrides?.filter ?? filter;
        const activeWindow = overrides?.windowSize ?? windowSize;
        trimBuffer();
        commitTail(activeFilter, activeWindow);
    }

    function handleEntry(entry: LiveLogEntry): void {
        const shouldFollow = !paused && isNearBottom(listRef.current);
        const previousBufferLength = buffer.entries.length;
        const matchesFilter = entryMatchesLevel(entry, filter);
        buffer.entries.push(entry);
        trimBuffer();

        if (!shouldFollow) {
            // A matching entry the projection did not pick up counts as new.
            if (matchesFilter) {
                setNewCount(count => count + 1);
            }
            return;
        }

        // A non-matching append below the retention limit cannot change the
        // filtered projection. Once trimming occurs, derive from the canonical
        // buffer because the removed entry may have matched the active filter.
        if (!matchesFilter && buffer.entries.length > previousBufferLength) {
            return;
        }
        commitTail(filter, windowSize);
    }

    useAsyncSubscription(
        client.subscribe,
        entry => handleEntry(entry),
        error => actions.reportError(error),
    );

    function handleScroll(): void {
        const nearBottom = isNearBottom(listRef.current);
        if (!paused && nearBottom && !wasNearBottomRef.current) {
            materializeTail();
            return;
        }
        wasNearBottomRef.current = nearBottom;
    }

    function changeFilter(next: string): void {
        setFilter(next);
        materializeTail({ filter: next });
    }

    function changePaused(next: boolean): void {
        setPaused(next);
        if (!next) {
            materializeTail();
        }
    }

    function showOlder(): void {
        if (windowSize >= LIVE_LOG_PANEL_MAX_WINDOW_SIZE) {
            return;
        }
        const next = Math.min(windowSize + LIVE_LOG_PANEL_WINDOW_GROW_STEP, LIVE_LOG_PANEL_MAX_WINDOW_SIZE);
        setWindowSize(next);
        materializeTail({ windowSize: next });
    }

    function copyRendered(): void {
        void actions.copyText(rendered.map(entry => formatEntryLine(entry)).join('\n'));
    }

    function clearRendered(): void {
        buffer.entries = [];
        setRendered([]);
        setTotalShown(0);
        setNewCount(0);
    }

    async function changeConsoleCapture(enabled: boolean): Promise<void> {
        // Optimistic: the boundary validation guarantees this method exists
        // whenever the toggle is shown; a persist failure rolls back.
        setConsoleCapture(enabled);
        try {
            await client.setConsoleCaptureEnabled?.(enabled);
        } catch (error) {
            setConsoleCapture(!enabled);
            actions.reportError(error);
        }
    }

    const statusParts = [`${tr('Showing')} ${rendered.length}/${totalShown}`];
    if (newCount > 0) {
        statusParts.push(`+${newCount} ${tr('new')}`);
    }
    if (paused) {
        statusParts.push(tr('Paused'));
    }
    const canShowOlder = windowSize < LIVE_LOG_PANEL_MAX_WINDOW_SIZE && totalShown > windowSize;

    return (
        <div className="tt-dev-logs-root">
            <header className="tt-dev-log-toolbar">
                <b>{tr(title)}</b>
                {showConsoleCapture && (
                    <DevLogToggle
                        checked={consoleCapture}
                        label={tr('Capture full console logs')}
                        onChange={enabled => void changeConsoleCapture(enabled)}
                    />
                )}
                <DevLogToggle
                    checked={paused}
                    label={tr('Pause')}
                    onChange={changePaused}
                />
                <DevLogButton label={tr('Jump to tail')} icon="fa-arrow-down" onClick={() => materializeTail()} />
                <DevLogButton label={tr('Copy')} icon="fa-copy" onClick={copyRendered} />
                <DevLogButton label={tr('Clear')} icon="fa-trash" onClick={clearRendered} />
            </header>

            <div className="tt-dev-log-levels">
                {LOG_LEVEL_OPTIONS.map(level => (
                    <button
                        key={level}
                        type="button"
                        className={`tt-dev-log-chip ${levelClass(level)}${level === filter ? ' active' : ''}`}
                        aria-pressed={level === filter}
                        onClick={() => changeFilter(level)}
                    >
                        {level}
                    </button>
                ))}
            </div>

            <div className="tt-dev-log-status">
                <small>{statusParts.join(' · ')}</small>
                {canShowOlder && (
                    <button type="button" className="tt-dev-log-chip" onClick={showOlder}>
                        {tr('Show older')}
                    </button>
                )}
            </div>

            <div ref={listRef} className="tt-dev-log-list" onScroll={handleScroll}>
                {rendered.length === 0 ? (
                    <div className="tt-dev-log-empty">
                        {filter === 'ALL' ? tr('No log entries yet') : tr('No entries match the selected level')}
                    </div>
                ) : rendered.map((entry, index) => (
                    <LogRow key={entry.id ?? `${entry.timestampMs}-${index}`} entry={entry} />
                ))}
            </div>
        </div>
    );
}
