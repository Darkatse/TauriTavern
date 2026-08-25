import { useCallback, useEffect, useRef, useState, type SyntheticEvent } from 'react';

import {
    initialPreviewState,
    type DevLogsActions,
    type DevLogsTranslate,
    type LlmApiLogIndexEntry,
    type LlmApiLogPreviewResult,
    type LlmApiLogsClient,
    type PreviewLoadState,
    type RawLoadState,
} from './DevLogsContract';
import { DevLogButton, TextPreviewSection } from './DevLogComponents';
import { useAsyncSubscription } from './useAsyncSubscription';
import { formatTime, formatTimestamp } from './log-utils';

type LlmIndexState = {
    entries: LlmApiLogIndexEntry[];
    selectedId: number | null;
};

/**
 * Selection fixup after a list change: the current selection survives when it
 * is still present; a selection trimmed away falls back to the oldest
 * remaining entry; an empty list clears it.
 */
function resolveSelection(entries: LlmApiLogIndexEntry[], selectedId: number | null): number | null {
    if (entries.length === 0) {
        return null;
    }
    if (selectedId !== null && entries.some(entry => entry.id === selectedId)) {
        return selectedId;
    }
    return entries.at(0)?.id ?? null;
}

function isFollowingLatest(state: LlmIndexState): boolean {
    return state.entries.length === 0 || state.selectedId === state.entries.at(-1)?.id;
}

function errorText(error: unknown): string {
    const message = (error as { message?: unknown } | null)?.message;
    if (typeof message === 'string' && message) {
        return message;
    }
    if (typeof error === 'string' && error) {
        return error;
    }
    return 'Unknown error';
}

type BodyField = 'request' | 'response';

function previewText(state: PreviewLoadState | null, field: BodyField, loadingText: string): string {
    if (!state || state.status === 'loading') {
        return loadingText;
    }
    if (state.status === 'error') {
        return '';
    }
    return field === 'request' ? state.value.requestReadable : state.value.responseReadable;
}

// A failed raw load surfaces once in the request pane.
function rawText(state: RawLoadState | null, field: BodyField, loadingText: string): string {
    if (!state || state.status === 'loading') {
        return loadingText;
    }
    if (state.status === 'error') {
        return field === 'request' ? state.error : '';
    }
    return field === 'request' ? state.value.requestRaw : state.value.responseRaw;
}

export type LlmApiLogsPanelProps = {
    initialKeep?: number;
    initialIndexEntries?: LlmApiLogIndexEntry[];
    initialPreview?: LlmApiLogPreviewResult | null;
    client: LlmApiLogsClient;
    actions: DevLogsActions;
    tr: DevLogsTranslate;
};

export function LlmApiLogsPanel({
    initialKeep,
    initialIndexEntries = [],
    initialPreview = null,
    client,
    actions,
    tr,
}: LlmApiLogsPanelProps) {
    const [indexState, setIndexState] = useState<LlmIndexState>(() => {
        const entries = initialIndexEntries.slice();
        return { entries, selectedId: entries.at(-1)?.id ?? null };
    });
    const [keep, setKeep] = useState(() => Number(initialKeep) || 1);
    const [keepInput, setKeepInput] = useState(() => String(Number(initialKeep) || 1));
    const [applyingKeep, setApplyingKeep] = useState(false);
    const [rawOpen, setRawOpen] = useState(false);
    const [preview, setPreview] = useState<PreviewLoadState | null>(() => initialPreviewState(initialPreview));
    const [raw, setRaw] = useState<RawLoadState | null>(null);

    // Stale-response guards for host reads, which offer no AbortSignal.
    const mountedRef = useRef(true);
    const previewEpochRef = useRef(0);
    const rawEpochRef = useRef(0);
    const selectedIdRef = useRef(indexState.selectedId);
    const reloadEventsRef = useRef<LlmApiLogIndexEntry[] | null>(null);

    useEffect(() => {
        mountedRef.current = true;
        return () => {
            mountedRef.current = false;
        };
    }, []);

    useEffect(() => {
        selectedIdRef.current = indexState.selectedId;
    }, [indexState.selectedId]);

    const commitPreview = useCallback((state: PreviewLoadState, epoch: number): PreviewLoadState => {
        if (mountedRef.current && epoch === previewEpochRef.current && state.id === selectedIdRef.current) {
            setPreview(state);
        }
        return state;
    }, []);

    const commitRaw = useCallback((state: RawLoadState, epoch: number): RawLoadState => {
        if (mountedRef.current && epoch === rawEpochRef.current && state.id === selectedIdRef.current) {
            setRaw(state);
        }
        return state;
    }, []);

    const fetchPreview = useCallback(async (id: number): Promise<PreviewLoadState> => {
        const epoch = ++previewEpochRef.current;
        try {
            const value = await client.getPreview(id);
            return commitPreview({ id, status: 'ready', value }, epoch);
        } catch (error) {
            return commitPreview({ id, status: 'error', error: errorText(error) }, epoch);
        }
    }, [client, commitPreview]);

    const fetchRaw = useCallback(async (id: number): Promise<RawLoadState> => {
        const epoch = ++rawEpochRef.current;
        try {
            const value = await client.getRaw(id);
            return commitRaw({ id, status: 'ready', value }, epoch);
        } catch (error) {
            return commitRaw({ id, status: 'error', error: errorText(error) }, epoch);
        }
    }, [client, commitRaw]);

    // Preview follows the selection. While no committed state matches the
    // selection the view derives its loading text, so this effect never needs
    // to set state synchronously.
    useEffect(() => {
        const id = indexState.selectedId;
        if (!id || (preview && preview.id === id)) {
            return;
        }
        void fetchPreview(id);
    }, [indexState.selectedId, preview, fetchPreview]);

    // Raw loads on demand while the disclosure is open; closing it drops the
    // projection from the toggle handler instead.
    useEffect(() => {
        const id = indexState.selectedId;
        if (!rawOpen || !id || (raw && raw.id === id)) {
            return;
        }
        void fetchRaw(id);
    }, [rawOpen, indexState.selectedId, raw, fetchRaw]);

    function handleIndexEntry(entry: LlmApiLogIndexEntry): void {
        reloadEventsRef.current?.push(entry);
        setIndexState(previous => {
            if (previous.entries.some(existing => existing.id === entry.id)) {
                return previous;
            }
            const following = isFollowingLatest(previous);
            const entries = [...previous.entries, entry].slice(-keep);
            if (following) {
                return { entries, selectedId: entries.at(-1)?.id ?? null };
            }
            return { entries, selectedId: resolveSelection(entries, previous.selectedId) };
        });
    }

    useAsyncSubscription(
        client.subscribeIndex,
        entry => handleIndexEntry(entry),
        error => actions.reportError(error),
    );

    const currentIndex = indexState.selectedId === null
        ? -1
        : indexState.entries.findIndex(entry => entry.id === indexState.selectedId);
    const currentEntry = currentIndex >= 0 ? (indexState.entries[currentIndex] ?? null) : null;
    const currentId = currentEntry?.id ?? 0;
    const hasEntries = indexState.entries.length > 0;

    const activePreview = preview && preview.id === currentId ? preview : null;
    const activeRaw = rawOpen && raw && raw.id === currentId ? raw : null;
    const loadingText = tr('Loading...');

    const metaSource = activePreview?.status === 'ready'
        ? activePreview.value
        : activePreview?.status === 'error'
            ? activePreview
            : currentEntry;
    const metaIsError = metaSource !== null && ('error' in metaSource || !metaSource.ok);
    const metaText = !metaSource
        ? ''
        : 'error' in metaSource
            ? String(metaSource.error)
            : `${metaSource.source}${metaSource.model ? ` (${metaSource.model})` : ''}\n${metaSource.endpoint}\n${tr('Duration')}: ${metaSource.durationMs}ms    ok: ${metaSource.ok}\n${formatTimestamp(metaSource.timestampMs)}`;

    const requestReadable = !currentId ? '' : previewText(activePreview, 'request', loadingText);
    const responseReadable = !currentId ? '' : previewText(activePreview, 'response', loadingText);
    const requestRaw = !rawOpen || !currentId ? '' : rawText(activeRaw, 'request', loadingText);
    const responseRaw = !rawOpen || !currentId ? '' : rawText(activeRaw, 'response', loadingText);

    const bodyTitle = (field: BodyField) => tr(field === 'request' ? 'Request body' : 'Response body');
    const rawViewerTitle = (field: BodyField) => `${tr('Raw JSON/SSE')} - ${bodyTitle(field)}`;

    function ensurePreview(id: number): Promise<PreviewLoadState> | PreviewLoadState {
        if (preview && preview.id === id && preview.status !== 'loading') {
            return preview;
        }
        return fetchPreview(id);
    }

    function ensureRaw(id: number): Promise<RawLoadState> | RawLoadState {
        if (raw && raw.id === id && raw.status !== 'loading') {
            return raw;
        }
        return fetchRaw(id);
    }

    async function copyPreviewText(field: BodyField): Promise<void> {
        if (!currentId) {
            return;
        }
        await actions.copyText(previewText(await ensurePreview(currentId), field, loadingText));
    }

    async function copyRawText(field: BodyField): Promise<void> {
        if (!currentId) {
            return;
        }
        await actions.copyText(rawText(await ensureRaw(currentId), field, loadingText));
    }

    async function expandPreviewText(field: BodyField): Promise<void> {
        if (!currentId) {
            return;
        }
        await actions.openTextViewer({
            title: bodyTitle(field),
            text: previewText(await ensurePreview(currentId), field, loadingText),
            wrap: 'soft',
        });
    }

    async function expandRawText(field: BodyField): Promise<void> {
        if (!currentId) {
            return;
        }
        await actions.openTextViewer({
            title: rawViewerTitle(field),
            text: rawText(await ensureRaw(currentId), field, loadingText),
            wrap: 'off',
        });
    }

    async function reloadCurrent(): Promise<void> {
        if (!currentId) {
            return;
        }
        setPreview({ id: currentId, status: 'loading' });
        await fetchPreview(currentId);
        if (rawOpen) {
            setRaw({ id: currentId, status: 'loading' });
            await fetchRaw(currentId);
        }
    }

    function selectRelative(delta: number): void {
        setIndexState(previous => {
            if (previous.entries.length === 0) {
                return previous;
            }
            const current = previous.entries.findIndex(entry => entry.id === previous.selectedId);
            const next = Math.max(0, Math.min(current + delta, previous.entries.length - 1));
            return { ...previous, selectedId: previous.entries[next]?.id ?? previous.selectedId };
        });
    }

    function selectById(id: number): void {
        setIndexState(previous => previous.entries.some(entry => entry.id === id)
            ? { ...previous, selectedId: id }
            : previous);
    }

    function handleRawToggle(event: SyntheticEvent<HTMLDetailsElement>): void {
        const open = event.currentTarget.open;
        setRawOpen(open);
        if (!open) {
            setRaw(null);
        }
    }

    // Two-phase keep commit: a failed setKeep leaves keep, entries and
    // selection untouched; once persisted, the new keep stays even when the
    // index reload fails.
    async function applyKeep(): Promise<void> {
        const parsed = Number(keepInput);
        if (!Number.isSafeInteger(parsed) || parsed <= 0) {
            actions.reportError(new Error(tr('LLM API keep must be a positive number')));
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
            setIndexState(previous => {
                const entries = previous.entries.slice(-parsed);
                return { entries, selectedId: resolveSelection(entries, previous.selectedId) };
            });

            let reloaded: LlmApiLogIndexEntry[];
            const reloadEvents: LlmApiLogIndexEntry[] = [];
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
            setIndexState(previous => {
                const seen = new Set(reloaded.map(entry => entry.id));
                const merged = reloaded.slice();
                for (const entry of reloadEvents) {
                    if (!seen.has(entry.id)) {
                        seen.add(entry.id);
                        merged.push(entry);
                    }
                }
                const entries = merged.slice(-parsed);
                const selectedId = isFollowingLatest(previous)
                    ? entries.at(-1)?.id ?? null
                    : resolveSelection(entries, previous.selectedId);
                return { entries, selectedId };
            });
        } finally {
            setApplyingKeep(false);
        }
    }

    return (
        <div className="tt-dev-logs-root tt-dev-llm-root">
            <header className="tt-dev-log-toolbar">
                <b>{tr('LLM API Logs')}</b>
                {hasEntries && (
                    <>
                        <DevLogButton label={tr('Prev')} icon="fa-chevron-left" onClick={() => selectRelative(-1)} />
                        <DevLogButton label={tr('Next')} icon="fa-chevron-right" onClick={() => selectRelative(1)} />
                        <select
                            className="text_pole tt-dev-log-entry-select"
                            value={indexState.selectedId ?? ''}
                            aria-label={tr('Log entry')}
                            onChange={event => selectById(Number(event.target.value))}
                        >
                            {[...indexState.entries].reverse().map(entry => (
                                <option key={entry.id} value={entry.id}>
                                    {`${entry.ok ? '✓' : '✗'} · ${formatTime(entry.timestampMs)} · ${entry.model ?? entry.source}`}
                                </option>
                            ))}
                        </select>
                        <DevLogButton label={tr('Reload')} icon="fa-rotate" onClick={() => void reloadCurrent()} />
                        <DevLogButton label={tr('Copy Request')} icon="fa-copy" onClick={() => void copyPreviewText('request')} />
                        <DevLogButton label={tr('Copy Response')} icon="fa-copy" onClick={() => void copyPreviewText('response')} />
                    </>
                )}
            </header>

            <div className="tt-dev-log-settings-row">
                <span>{tr('LLM API keep')}</span>
                <input
                    className="text_pole tt-dev-log-keep-input"
                    type="number"
                    min="1"
                    step="1"
                    value={keepInput}
                    aria-label={tr('LLM API keep')}
                    onChange={event => setKeepInput(event.target.value)}
                />
                <DevLogButton
                    label={tr('Apply')}
                    icon="fa-check"
                    disabled={applyingKeep}
                    onClick={() => void applyKeep()}
                />
            </div>

            <small className="tt-dev-log-note">{tr('LLM API logs capture prompt/response bodies.')}</small>

            {!hasEntries ? (
                <div className="tt-dev-log-empty">{tr('No LLM API entries yet')}</div>
            ) : (
                <>
                    <div className={`tt-dev-log-meta${metaIsError ? ' tt-dev-log-meta-error' : ''}`}>{metaText}</div>

                    <TextPreviewSection
                        title={tr('Request body')}
                        text={requestReadable}
                        placeholder={tr('Request body')}
                        rows={10}
                        onExpand={() => void expandPreviewText('request')}
                    />
                    <TextPreviewSection
                        title={tr('Response body')}
                        text={responseReadable}
                        placeholder={tr('Response body')}
                        rows={14}
                        onExpand={() => void expandPreviewText('response')}
                    />

                    <details className="tt-dev-log-raw" open={rawOpen} onToggle={handleRawToggle}>
                        <summary>{tr('Raw JSON/SSE')}</summary>
                        <div className="tt-dev-log-raw-body">
                            <div className="tt-dev-log-toolbar compact">
                                <DevLogButton label={tr('Copy Raw Request')} icon="fa-copy" onClick={() => void copyRawText('request')} />
                                <DevLogButton label={tr('Copy Raw Response')} icon="fa-copy" onClick={() => void copyRawText('response')} />
                            </div>
                            <TextPreviewSection
                                title={tr('Request body')}
                                viewerTitle={rawViewerTitle('request')}
                                text={requestRaw}
                                placeholder={tr('Request body')}
                                rows={10}
                                wrap="off"
                                onExpand={() => void expandRawText('request')}
                            />
                            <TextPreviewSection
                                title={tr('Response body')}
                                viewerTitle={rawViewerTitle('response')}
                                text={responseRaw}
                                placeholder={tr('Response body')}
                                rows={14}
                                wrap="off"
                                onExpand={() => void expandRawText('response')}
                            />
                        </div>
                    </details>
                </>
            )}
        </div>
    );
}
