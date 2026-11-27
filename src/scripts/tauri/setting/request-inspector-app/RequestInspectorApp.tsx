import { StrictMode, useDeferredValue, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';

import type { RawPayload, Reveal } from './PayloadView';
import {
    validateRequestInspectorBoundary,
    type RequestInspectorClient,
    type RequestInspectorHandle,
    type RequestInspectorMountOptions,
    type RequestView,
    type Selection,
    type Translate,
} from './RequestInspectorContract';
import { SubjectView, type Subject } from './SubjectView';
import { MorePanel, Toolbar, type Mode, type Panel } from './Toolbar';
import type { Comparable } from './compare';
import { type BaseRef, baseKey, baseOptions, nearestBase } from './compare-base';
import { buildCurl, hasInlineMedia } from './curl';
import { mapLoad, useLoad, type LoadState } from './useLoad';
import { useLogIndex } from './useLogIndex';

const MODE_KEY = 'tt-request-inspector-mode';

/** Structure or payload is a per-device habit; where storage is unavailable it lasts for this popup. */
function readMode(): Mode {
    try {
        return localStorage.getItem(MODE_KEY) === 'payload' ? 'payload' : 'structure';
    } catch {
        return 'structure';
    }
}

function writeMode(mode: Mode): void {
    try {
        localStorage.setItem(MODE_KEY, mode);
    } catch {
        // Kept in state only.
    }
}

function requestOf(subject: Subject): RequestView {
    return (subject.kind === 'preview' ? subject.preview : subject.log).view.request;
}

/** What can be copied of the shown request. A preview holds its texts; a log reads them when asked. */
function copyOptions(subject: Subject, client: RequestInspectorClient, tr: Translate): {
    label: string;
    text: () => string | Promise<string>;
}[] {
    const { http } = requestOf(subject);
    const log = subject.kind === 'log' ? subject.log : null;
    const requestRaw = subject.kind === 'preview'
        ? () => subject.preview.requestRaw
        : async () => (await client.getRaw(subject.log.id)).requestRaw;
    return [
        { label: tr('Copy request JSON'), text: requestRaw },
        ...(http?.transport === 'http'
            ? [{ label: tr('Copy as cURL'), text: async () => buildCurl(http, await requestRaw()) ?? '' }]
            : []),
        {
            label: tr('Copy request text'),
            text: subject.kind === 'preview'
                ? () => subject.preview.requestReadable
                : async () => (await client.getLogText(subject.log.id)).requestReadable,
        },
        ...(log && log.responseRawKind !== null ? [
            { label: tr('Copy response text'), text: async () => (await client.getLogText(log.id)).responseReadable },
            { label: tr('Copy raw response'), text: async () => (await client.getRaw(log.id)).responseRaw },
        ] : []),
    ];
}

function RequestInspectorApp({ options }: { options: RequestInspectorMountOptions }) {
    const { client, actions, tr } = options;
    const index = useLogIndex(options);
    const { selection } = index;
    const [mode, setMode] = useState(readMode);
    const [pickedBase, setPickedBase] = useState<BaseRef | null>(null);
    const [reveal, setReveal] = useState<Reveal | null>(null);
    const [panel, setPanel] = useState<Panel>(null);
    const [query, setQuery] = useState('');
    const needle = useDeferredValue(query.trim().toLowerCase());

    const logId = selection.kind === 'log' ? selection.id : null;
    const preview = useLoad(selection.kind === 'preview' ? 'preview' : null, () => client.preview(), actions.errorText);
    const log = useLoad(logId, client.getLogView, actions.errorText);
    const logRaw = useLoad(mode === 'payload' ? logId : null, client.getRaw, actions.errorText);

    const subject = useMemo((): LoadState<Subject> | null => {
        if (preview) {
            return mapLoad(preview, value => ({ kind: 'preview', preview: value.preview }));
        }
        return log && mapLoad(log, value => ({ kind: 'log', log: value }));
    }, [preview, log]);

    const raw = useMemo((): LoadState<RawPayload> => {
        if (preview) {
            return mapLoad(preview, value => ({ requestRaw: value.preview.requestRaw, responseRaw: null, responseRawKind: null }));
        }
        return mapLoad(logRaw ?? { status: 'loading' }, ({ requestRaw, responseRaw, responseRawKind }) => ({
            requestRaw,
            responseRaw: responseRawKind === null ? null : responseRaw,
            responseRawKind,
        }));
    }, [preview, logRaw]);

    const assembled = preview?.status === 'ready' ? preview.value : null;
    const earlier = assembled?.earlier ?? null;
    const bases = baseOptions(selection, index.entries, earlier, tr);
    const madeAt = selection.kind === 'preview'
        ? assembled?.timestampMs
        : index.entries.find(entry => entry.id === selection.id)?.timestampMs;
    const base = (pickedBase && bases.find(option => option.key === baseKey(pickedBase)))
        ?? nearestBase(bases, madeAt ?? Infinity);
    const baseKind = base?.ref.kind ?? null;
    const baseLog = useLoad(mode === 'compare' && base?.ref.kind === 'log' ? base.ref.id : null, client.getLogView, actions.errorText);
    const baseRequest = useMemo((): LoadState<Comparable> | null => {
        if (baseKind === 'earlier') {
            return earlier && { status: 'ready', value: { view: earlier.view, assembly: earlier } };
        }
        return baseLog && mapLoad(baseLog, value => ({ view: value.view, assembly: null }));
    }, [baseKind, earlier, baseLog]);

    const select = (next: Selection) => {
        setReveal(null);
        setPickedBase(null);
        index.select(next);
    };
    const changeMode = (next: Mode) => {
        setReveal(null);
        setMode(next);
        if (next !== 'compare') {
            writeMode(next);
        }
    };
    const copy = (produce: () => string | Promise<string>) => () => {
        void (async () => {
            try {
                await actions.copyText(await produce());
            } catch (error) {
                actions.reportError(error);
            }
        })();
    };

    const shown = subject?.status === 'ready' ? subject.value : null;
    const copies = shown
        ? copyOptions(shown, client, tr).map(({ label, text }) => ({ label, run: copy(text) }))
        : [];
    const inlineMedia = shown !== null && hasInlineMedia(requestOf(shown));

    return (
        <div className="tt-ri-root">
            <Toolbar
                entries={index.entries}
                selection={selection}
                onSelect={select}
                mode={mode}
                onMode={changeMode}
                bases={bases}
                base={base?.key ?? null}
                onBase={setPickedBase}
                panel={panel}
                onPanel={(next) => {
                    setPanel(next);
                    setQuery('');
                }}
                query={query}
                onQuery={setQuery}
                more={(
                    <MorePanel
                        copies={copies}
                        hint={inlineMedia ? tr('Inline media is stored as a placeholder, so a copied request carries the placeholder.') : ''}
                        keepInput={index.keepInput}
                        onKeepInput={index.setKeepInput}
                        applyingKeep={index.applyingKeep}
                        onApplyKeep={() => void index.applyKeep()}
                        tr={tr}
                    />
                )}
                tr={tr}
            />
            <div className="tt-ri-scroll">
                {subject === null && <p className="tt-ri-empty">{tr('No requests have been sent yet')}</p>}
                {subject?.status === 'loading' && (
                    <p className="tt-ri-empty">
                        {selection.kind === 'preview' ? tr('Assembling the preview...') : tr('Loading...')}
                    </p>
                )}
                {subject?.status === 'error' && (
                    <div className="tt-ri-empty"><p className="tt-ri-error">{subject.error}</p></div>
                )}
                {shown && (
                    <SubjectView
                        key={shown.kind === 'preview' ? 'preview' : shown.log.id}
                        subject={shown}
                        base={baseRequest}
                        raw={raw}
                        mode={mode}
                        reveal={reveal}
                        onReveal={(target) => {
                            setReveal(target);
                            setMode('payload');
                        }}
                        needle={needle}
                        onCopy={text => void actions.copyText(text)}
                        onViewText={viewer => void actions.openTextViewer(viewer)}
                        tr={tr}
                    />
                )}
            </div>
        </div>
    );
}

export function mountTauriTavernRequestInspectorApp(mount: unknown, options: unknown): RequestInspectorHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern request inspector mount element is required');
    }
    validateRequestInspectorBoundary(options);

    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <RequestInspectorApp options={options} />
        </StrictMode>,
    );

    return {
        unmount: () => root.unmount(),
    };
}
