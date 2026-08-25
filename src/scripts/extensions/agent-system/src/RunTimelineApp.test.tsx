import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import type { AgentSystemSettings } from './AgentSystemEntryController';
import { RunTimelineApp } from './RunTimelineApp';
import { createRunTimelineController } from './RunTimelineController';
import type { ActiveTimelineOptions, RunTimelineController } from './RunTimelineContract';

const tr = (key: string): string => key;
const controllers: RunTimelineController[] = [];

function settings(agentModeEnabled = true): AgentSystemSettings {
    return {
        agentModeEnabled,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId: 'default-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    };
}

function fileEvent(seq: number): TauriTavernAgentRunEvent {
    return {
        seq,
        id: `event-${seq}`,
        runId: 'run-1',
        timestamp: '2026-01-01T00:00:00Z',
        level: 'info',
        type: 'workspace_file_written',
        payload: { path: `file-${seq}.txt`, chars: 5, words: 1 },
    };
}

afterEach(() => {
    cleanup();
    controllers.splice(0).forEach(controller => controller.dispose());
    Reflect.deleteProperty(window, '__TAURITAVERN__');
});

test('active hide/show and timeline/detail switches preserve the mounted event scroller', async () => {
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: {
            api: {
                agent: {
                    readWorkspaceFile: ({ path }: { path: string }) => Promise.resolve({
                        path,
                        text: 'hello',
                        chars: 5,
                        words: 1,
                        sha256: 'hash',
                    }),
                },
            },
        },
    });
    let settingsListener: ((value: AgentSystemSettings) => void) | null = null;
    const deps: ActiveTimelineOptions['deps'] = {
        readEvents: () => Promise.resolve({
            events: [fileEvent(1)],
            timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
        }),
        reportError: error => { throw error; },
        tr,
        loadSettings: () => Promise.resolve(settings()),
        patchSettings: (current, patch) => Promise.resolve({ ...current, ...patch }),
        subscribeSettings: listener => {
            settingsListener = listener;
            return () => undefined;
        },
        getActiveRun: () => ({ runId: 'run-1', generationType: 'normal' }),
        subscribeRunState: () => () => undefined,
        subscribeRunEvents: () => () => undefined,
        retryFailure: () => Promise.resolve(),
    };
    const controller = createRunTimelineController({ mode: 'active', deps });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());

    const root = document.getElementById('ttas_agent_run_timeline');
    expect(root?.style.display).toBe('');
    await userEvent.setup().click(screen.getByRole('button', { name: 'expandTimeline' }));
    const scroller = root?.querySelector('.ttas-run-event-scroll');
    expect(scroller).not.toBeNull();

    await userEvent.setup().click(screen.getByRole('button', { name: 'showTimelineDetails' }));
    await waitFor(() => expect(root?.querySelector('.ttas-run-view-details')).not.toBeNull());
    expect(root?.querySelector<HTMLElement>('.ttas-run-view-events')?.style.display).toBe('none');
    expect(root?.querySelector('.ttas-run-event-scroll')).toBe(scroller);

    act(() => settingsListener?.(settings(false)));
    expect(root?.style.display).toBe('none');
    act(() => settingsListener?.(settings(true)));
    expect(root?.style.display).toBe('');
    expect(root?.dataset.ttasView).toBe('details');
    expect(root?.querySelector('.ttas-run-event-scroll')).toBe(scroller);

    const showTimeline = screen.getAllByRole('button', { name: 'showTimelineEvents' })[0];
    if (!showTimeline) throw new Error('expected the timeline view action');
    await userEvent.setup().click(showTimeline);
    expect(root?.querySelector<HTMLElement>('.ttas-run-view-events')?.style.display).toBe('');
    expect(root?.querySelector('.ttas-run-event-scroll')).toBe(scroller);
});

test('the React event list renders only the virtual window while the controller retains full history', async () => {
    const controller = createRunTimelineController({
        mode: 'history',
        rootId: 'history-window',
        run: { runId: 'run-1' },
        requestClose: () => undefined,
        deps: {
            readEvents: () => Promise.resolve({
                events: Array.from({ length: 240 }, (_, index) => fileEvent(index + 1)),
                timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
            }),
            reportError: error => { throw error; },
            tr,
        },
    });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());

    expect(controller.getSnapshot().displayItems).toHaveLength(240);
    expect(document.querySelectorAll('#history-window li.ttas-run-event').length).toBeLessThan(240);
    expect(document.querySelectorAll('#history-window li.ttas-run-event').length).toBeGreaterThan(0);
});

test('SubAgent tray opens and closes its native dialog through controller-local state', async () => {
    const showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal');
    const closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
        configurable: true,
        value(this: HTMLDialogElement) { this.open = true; },
    });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
        configurable: true,
        value(this: HTMLDialogElement) {
            this.open = false;
            this.dispatchEvent(new Event('close'));
        },
    });
    const reads: Array<{ invocationId?: string }> = [];
    const controller = createRunTimelineController({
        mode: 'active',
        deps: {
            readEvents: input => {
                reads.push(input);
                return Promise.resolve(input.invocationId ? { events: [] } : {
                    events: [],
                    timelineProjection: {
                        foregroundInvocationIds: ['inv_root'],
                        invocations: [{
                            invocationId: 'inv-child',
                            parentInvocationId: 'inv_root',
                            profileId: 'critic',
                            kind: 'subagent',
                            status: 'running',
                            exitPolicy: 'task_return_required',
                            createdAt: '2026-01-01T00:00:00Z',
                            updatedAt: '2026-01-01T00:00:00Z',
                        }],
                        delegationEdges: [{
                            taskId: 'task-1',
                            sourceInvocationId: 'inv_root',
                            targetInvocationId: 'inv-child',
                            targetProfileId: 'critic',
                            workspaceKey: 'critic',
                            continuation: 'return_to_parent',
                            status: 'running',
                            createdAt: '2026-01-01T00:00:00Z',
                            updatedAt: '2026-01-01T00:00:00Z',
                        }],
                    },
                });
            },
            reportError: error => { throw error; },
            tr,
            loadSettings: () => Promise.resolve(settings()),
            patchSettings: (current, patch) => Promise.resolve({ ...current, ...patch }),
            subscribeSettings: () => () => undefined,
            getActiveRun: () => ({ runId: 'run-1' }),
            subscribeRunState: () => () => undefined,
            subscribeRunEvents: () => () => undefined,
            retryFailure: () => Promise.resolve(),
        },
    });
    controllers.push(controller);
    try {
        render(<RunTimelineApp controller={controller} tr={tr} />);
        await act(() => controller.init());
        const user = userEvent.setup();
        await user.click(screen.getByRole('button', { name: 'expandTimeline' }));
        await user.click(screen.getByTitle('timelineExpandSubAgents'));
        await user.click(screen.getByRole('button', { name: /critic/ }));
        const dialog = document.querySelector<HTMLDialogElement>('dialog.ttas-subagent-dialog');
        await waitFor(() => expect(dialog?.open).toBe(true));
        expect(reads.at(-1)?.invocationId).toBe('inv-child');
        if (!dialog) throw new Error('expected the SubAgent dialog');
        await user.click(within(dialog).getByRole('button', { name: 'close' }));
        expect(dialog.open).toBe(false);
        expect(controller.getSnapshot().subAgent.open).toBe(false);
    } finally {
        if (showModalDescriptor) Object.defineProperty(HTMLDialogElement.prototype, 'showModal', showModalDescriptor);
        else Reflect.deleteProperty(HTMLDialogElement.prototype, 'showModal');
        if (closeDescriptor) Object.defineProperty(HTMLDialogElement.prototype, 'close', closeDescriptor);
        else Reflect.deleteProperty(HTMLDialogElement.prototype, 'close');
    }
});
