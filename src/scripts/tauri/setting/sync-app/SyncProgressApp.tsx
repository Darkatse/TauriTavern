import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { formatBytesValue } from './format.js';

/**
 * Sync progress popup island. `setting-panel/sync-listeners.js` owns the sync:job
 * event listener and the popup lifecycle, and pushes state through `update()`;
 * this root only projects the latest snapshot into DOM and holds no state of
 * its own beyond the current projection.
 */

export type SyncProgressPayload = {
    direction?: string | null;
    phase?: string | null;
    files_done?: number | null;
    files_total?: number | null;
    bytes_done?: number | null;
    bytes_total?: number | null;
    current_path?: string | null;
};

export type SyncProgressState = {
    title?: string | null;
    payload?: SyncProgressPayload | null;
};

export type SyncProgressOptions = SyncProgressState & {
    tr: (key: string) => string;
};

export type SyncProgressHandle = {
    update(next: SyncProgressState): void;
    unmount(): void;
};

type SyncProgressViewProps = {
    title: string;
    payload: SyncProgressPayload;
    tr: (key: string) => string;
};

function SyncProgressView({ title, payload, tr }: SyncProgressViewProps) {
    const direction = payload.direction || null;
    const phase = payload.phase || 'Starting';
    const phaseText = direction
        ? `${tr('Phase')}: ${tr(direction)} / ${tr(phase)}`
        : `${tr('Phase')}: ${tr(phase)}`;
    const countsText = `${tr('Files')}: ${Number(payload.files_done) || 0}/${Number(payload.files_total) || 0}`;
    const bytesText = `${tr('Bytes')}: ${formatBytesValue(payload.bytes_done)}/${formatBytesValue(payload.bytes_total)}`;
    const currentPath = payload.current_path || '';

    return (
        <div className="tt-sync-progress-root">
            <b>{tr(title)}</b>
            <div>{phaseText}</div>
            <div>{countsText}</div>
            <div>{bytesText}</div>
            <div className="tt-sync-progress-current">
                {currentPath ? `${tr('Current')}: ${currentPath}` : ''}
            </div>
        </div>
    );
}

export function mountTauriTavernSyncProgressApp(
    mount: unknown,
    options: SyncProgressOptions,
): SyncProgressHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync progress mount element is required');
    }
    const tr = options?.tr;
    if (typeof tr !== 'function') {
        throw new Error('TauriTavern Sync progress translator is required');
    }

    let current = {
        title: options.title || 'Sync progress',
        payload: options.payload || {},
    };
    const root = createRoot(mount);

    function render(): void {
        root.render(
            <StrictMode>
                <SyncProgressView title={current.title} payload={current.payload} tr={tr} />
            </StrictMode>,
        );
    }

    render();

    return {
        update(next) {
            // The listener pushes whole snapshots; only provided fields replace
            // the projection, everything else keeps its previous value.
            if (next.title) {
                current = { ...current, title: next.title };
            }
            if (next.payload) {
                current = { ...current, payload: next.payload };
            }
            render();
        },
        unmount() {
            root.unmount();
        },
    };
}
