import { StrictMode } from 'react';
import { createRoot, type Root } from 'react-dom/client';

import type { AgentSystemSettings } from './AgentSystemEntryController';
import { RunTimelineApp } from './RunTimelineApp';
import { createRunTimelineController } from './RunTimelineController';
import type {
    RunTimelineController,
    TimelineReadInput,
} from './RunTimelineContract';
import { errorText, requireAgentApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import {
    getActiveAgentRun,
    subscribeAgentRunEvents,
    subscribeAgentRunState,
} from '../../../tauritavern/agent/agent-run-controller.js';
import { retryAgentRunFailure } from '../../../tauritavern/agent/agent-run-retry.js';

const MOUNT_ID = 'ttas_agent_run_timeline_mount';
let historyTimelineDialogCounter = 0;

function reportTimelineError(error: unknown): void {
    console.error('[AgentSystem] Timeline operation failed', error);
    window.toastr?.error?.(errorText(error), tr('agentSystem'));
}

function readEvents(input: TimelineReadInput) {
    return requireAgentApi().readEvents(input);
}

export function openAgentRunTimelineDialog(run: TauriTavernAgentRunSummary): void {
    const runId = run.runId.trim();
    if (!runId) throw new Error('Agent run id is required.');
    if (typeof HTMLDialogElement === 'undefined') throw new Error(tr('runHistoryDialogUnsupported'));

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') throw new Error(tr('runHistoryDialogUnsupported'));
    dialog.className = 'ttas-dialog ttas-run-history-dialog';
    dialog.dataset.ttMobileSurface = 'fullscreen-window';
    const mount = document.createElement('div');
    mount.className = 'ttas-run-history-dialog-mount';
    dialog.append(mount);
    document.body.append(dialog);

    let root: Root | null = null;
    let controller: RunTimelineController | null = null;
    let disposed = false;
    const cleanup = () => {
        if (disposed) return;
        disposed = true;
        controller?.dispose();
        root?.unmount();
        dialog.remove();
    };
    const close = () => {
        if (dialog.open) dialog.close();
        else cleanup();
    };
    dialog.addEventListener('cancel', event => {
        event.preventDefault();
        close();
    });
    dialog.addEventListener('close', cleanup, { once: true });

    controller = createRunTimelineController({
        mode: 'history',
        rootId: `ttas_agent_run_timeline_history_${++historyTimelineDialogCounter}`,
        run: { ...run, runId },
        requestClose: close,
        deps: { readEvents, reportError: reportTimelineError, tr },
    });
    root = createRoot(mount);
    root.render(
        <StrictMode>
            <RunTimelineApp controller={controller} tr={tr} />
        </StrictMode>,
    );
    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }
    void controller.init().catch(error => queueMicrotask(() => { throw error; }));
}

export async function mountAgentRunTimelinePanel(): Promise<void> {
    const sendForm = document.getElementById('send_form');
    if (!(sendForm instanceof HTMLElement) || !(sendForm.parentElement instanceof HTMLElement)) {
        throw new Error(tr('sendFormNotFound'));
    }
    if (document.getElementById(MOUNT_ID)) return;

    const mount = document.createElement('div');
    mount.id = MOUNT_ID;
    mount.className = 'ttas-run-timeline-mount';
    sendForm.parentElement.insertBefore(mount, sendForm);
    const controller = createRunTimelineController({
        mode: 'active',
        deps: {
            readEvents,
            reportError: reportTimelineError,
            tr,
            loadSettings: () => loadSettings() as Promise<AgentSystemSettings>,
            patchSettings: (current, patch) => patchSettings(current, patch) as Promise<AgentSystemSettings>,
            subscribeSettings: listener => subscribeSettings(listener),
            getActiveRun: () => getActiveAgentRun(),
            subscribeRunState: listener => subscribeAgentRunState(listener),
            subscribeRunEvents: listener => subscribeAgentRunEvents(listener),
            retryFailure: input => retryAgentRunFailure(input),
        },
    });
    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <RunTimelineApp controller={controller} tr={tr} />
        </StrictMode>,
    );
    try {
        await controller.init();
    } catch (error) {
        controller.dispose();
        root.unmount();
        mount.remove();
        throw error;
    }
}
