import { useRef, useState } from 'react';

import { useAsyncSubscription } from '../shared/useAsyncSubscription';
import type { LogIndexEntry, RequestInspectorMountOptions, Selection } from './RequestInspectorContract';

type IndexState = {
    /** Oldest first, at most `keep` of them. */
    entries: LogIndexEntry[];
    selection: Selection;
};

/** The newest log is selected, or the log is selected with nothing recorded yet. */
function isFollowingLatest({ entries, selection }: IndexState): boolean {
    return selection.kind === 'log' && (entries.length === 0 || selection.id === entries.at(-1)?.id);
}

/** A selected log stays while it is listed and falls back to the oldest entry once trimmed away. */
function resolve(entries: LogIndexEntry[], selection: Selection): Selection {
    if (selection.kind === 'preview' || entries.some(entry => entry.id === selection.id)) {
        return selection;
    }
    return { kind: 'log', id: entries.at(0)?.id ?? null };
}

/** Selection after entries arrived: a log that followed the latest keeps following. */
function settle(entries: LogIndexEntry[], previous: IndexState): Selection {
    return isFollowingLatest(previous)
        ? { kind: 'log', id: entries.at(-1)?.id ?? null }
        : resolve(entries, previous.selection);
}

/** The kept logs, which request is selected, and how many logs are kept. */
export function useLogIndex({ client, actions, tr, keep: initialKeep, entries: initialEntries, select: initial }: RequestInspectorMountOptions) {
    const [state, setState] = useState<IndexState>(() => ({
        entries: initialEntries.slice(),
        selection: initial === 'preview' ? { kind: 'preview' } : { kind: 'log', id: initialEntries.at(-1)?.id ?? null },
    }));
    const [keep, setKeep] = useState(initialKeep);
    const [keepInput, setKeepInput] = useState(String(initialKeep));
    const [applyingKeep, setApplyingKeep] = useState(false);
    const reloadEventsRef = useRef<LogIndexEntry[] | null>(null);

    useAsyncSubscription(
        client.subscribeIndex,
        (entry) => {
            reloadEventsRef.current?.push(entry);
            setState(previous => {
                if (previous.entries.some(existing => existing.id === entry.id)) {
                    return previous;
                }
                const entries = [...previous.entries, entry].slice(-keep);
                return { entries, selection: settle(entries, previous) };
            });
        },
        error => actions.reportError(error),
    );

    function select(selection: Selection): void {
        setState(previous => resolve(previous.entries, selection) === selection ? { ...previous, selection } : previous);
    }

    // Two-phase keep commit: a failed setKeep leaves keep, entries and
    // selection untouched; once persisted, the new keep stays even when the
    // index reload fails.
    async function applyKeep(): Promise<void> {
        const parsed = Number(keepInput);
        if (!Number.isSafeInteger(parsed) || parsed <= 0) {
            actions.reportError(new Error(tr('The number of kept requests must be a positive whole number')));
            return;
        }

        setApplyingKeep(true);
        try {
            try {
                await client.setKeep(parsed);
            } catch (error) {
                actions.reportError(error);
                return;
            }

            setKeep(parsed);
            setKeepInput(String(parsed));
            setState(previous => {
                const entries = previous.entries.slice(-parsed);
                return { entries, selection: resolve(entries, previous.selection) };
            });

            let reloaded: LogIndexEntry[];
            const reloadEvents: LogIndexEntry[] = [];
            reloadEventsRef.current = reloadEvents;
            try {
                reloaded = await client.index({ limit: parsed });
            } catch (error) {
                reloadEventsRef.current = null;
                actions.reportError(error);
                return;
            }
            reloadEventsRef.current = null;
            // Events that arrived during the reload may overlap the snapshot
            // window; keep host snapshot order, then arrival order.
            setState(previous => {
                const seen = new Set(reloaded.map(entry => entry.id));
                const merged = reloaded.slice();
                for (const entry of reloadEvents) {
                    if (!seen.has(entry.id)) {
                        seen.add(entry.id);
                        merged.push(entry);
                    }
                }
                const entries = merged.slice(-parsed);
                return { entries, selection: settle(entries, previous) };
            });
        } finally {
            setApplyingKeep(false);
        }
    }

    return {
        entries: state.entries,
        selection: state.selection,
        select,
        keepInput,
        setKeepInput,
        applyingKeep,
        applyKeep,
    };
}
