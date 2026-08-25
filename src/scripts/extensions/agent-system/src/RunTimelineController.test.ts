import { expect, rs, test } from '@rstest/core';

import type { AgentSystemSettings } from './settings-store';
import { createRunTimelineController } from './RunTimelineController';
import type {
    ActiveTimelineOptions,
    TimelineProjection,
    TimelineReadInput,
    TimelineReadResult,
} from './RunTimelineContract';

const tr = (key: string, params: Record<string, unknown> = {}) => [
    key,
    ...Object.entries(params).map(([name, value]) => `${name}=${JSON.stringify(value) ?? ''}`),
].join(' ');

function settings(overrides: Partial<AgentSystemSettings> = {}): AgentSystemSettings {
    return {
        agentModeEnabled: true,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId: 'default-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
        ...overrides,
    };
}

function event(
    seq: number,
    type = 'workspace_file_written',
    payload: Record<string, unknown> = { path: `file-${seq}.txt` },
    runId = 'run-1',
): TauriTavernAgentRunEvent {
    return {
        seq,
        id: `event-${runId}-${seq}`,
        runId,
        timestamp: '2026-01-01T00:00:00Z',
        level: type === 'run_failed' ? 'error' : 'info',
        type,
        payload,
    };
}

function emptyProjection(): TimelineProjection {
    return { foregroundInvocationIds: [], invocations: [], delegationEdges: [] };
}

async function flushMicrotasks(): Promise<void> {
    for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

function subAgentProjection(): TimelineProjection {
    return {
        foregroundInvocationIds: ['inv_root'],
        invocations: [
            {
                invocationId: 'inv-child',
                parentInvocationId: 'inv_root',
                profileId: 'critic',
                kind: 'subagent',
                status: 'running',
                exitPolicy: 'task_return_required',
                createdAt: '2026-01-01T00:00:00Z',
                updatedAt: '2026-01-01T00:00:00Z',
            },
        ],
        delegationEdges: [
            {
                taskId: 'task-1',
                sourceInvocationId: 'inv_root',
                targetInvocationId: 'inv-child',
                targetProfileId: 'critic',
                workspaceKey: 'critic',
                continuation: 'return_to_parent',
                status: 'running',
                resultRef: '',
                error: '',
                createdAt: '2026-01-01T00:00:00Z',
                updatedAt: '2026-01-01T00:00:00Z',
            },
        ],
    };
}

function activeHarness(overrides: Partial<ActiveTimelineOptions['deps']> = {}) {
    const state = {
        order: [] as string[],
        settingsListener: null as ((value: AgentSystemSettings) => void) | null,
        runStateListener: null as ((value: {
            activeRun: { runId: string; generationType?: string } | null;
            lastEvent: TauriTavernAgentRunEvent | null;
        }) => void) | null,
        eventListener: null as ((value: TauriTavernAgentRunEvent) => void) | null,
        unsubscribed: 0,
        patches: [] as Array<Partial<AgentSystemSettings>>,
        retries: 0,
        errors: [] as unknown[],
    };
    const unsubscribe = () => { state.unsubscribed += 1; };
    const deps: ActiveTimelineOptions['deps'] = {
        readEvents: () => Promise.resolve({ events: [], timelineProjection: emptyProjection() }),
        reportError: error => state.errors.push(error),
        tr,
        loadSettings: () => {
            state.order.push('load-settings');
            return Promise.resolve(settings());
        },
        patchSettings: (current, patch) => {
            state.patches.push(patch);
            return Promise.resolve({ ...current, ...patch });
        },
        subscribeSettings: listener => {
            state.order.push('subscribe-settings');
            state.settingsListener = listener;
            return unsubscribe;
        },
        getActiveRun: () => {
            state.order.push('get-active-run');
            return { runId: 'run-1', generationType: 'normal' };
        },
        subscribeRunState: listener => {
            state.order.push('subscribe-run-state');
            state.runStateListener = listener;
            return unsubscribe;
        },
        subscribeRunEvents: listener => {
            state.order.push('subscribe-run-events');
            state.eventListener = listener;
            return unsubscribe;
        },
        retryFailure: () => {
            state.retries += 1;
            return Promise.resolve();
        },
        ...overrides,
    };
    return { controller: createRunTimelineController({ mode: 'active', deps }), state };
}

test('active mode subscribes before reading the active snapshot and deduplicates history/live overlap', async () => {
    const history = { resolve: null as ((result: TimelineReadResult) => void) | null };
    const { controller, state } = activeHarness({
        readEvents: () => new Promise(resolve => { history.resolve = resolve; }),
    });
    const initializing = controller.init();
    await Promise.resolve();
    await Promise.resolve();
    expect(state.order.indexOf('subscribe-settings')).toBeLessThan(state.order.indexOf('get-active-run'));
    expect(state.order.indexOf('subscribe-run-state')).toBeLessThan(state.order.indexOf('get-active-run'));
    expect(state.order.indexOf('subscribe-run-events')).toBeLessThan(state.order.indexOf('get-active-run'));

    state.eventListener?.(event(1));
    history.resolve?.({ events: [event(1), event(2)], timelineProjection: emptyProjection() });
    await initializing;
    expect(controller.getSnapshot().displayItems.map(item => item.seq)).toEqual([1, 2]);
    expect(controller.getSnapshot().visible).toBe(true);

    state.settingsListener?.(settings({ agentModeEnabled: false }));
    expect(controller.getSnapshot().visible).toBe(false);
    controller.dispose();
    expect(state.unsubscribed).toBe(3);
});

test('projection refresh coalesces structural events into one trailing read', async () => {
    rs.useFakeTimers();
    try {
        const reads: TimelineReadInput[] = [];
        const { controller, state } = activeHarness({
            readEvents: input => {
                reads.push(input);
                return Promise.resolve({ events: [], timelineProjection: emptyProjection() });
            },
        });
        await controller.init();
        state.eventListener?.(event(1, 'agent_delegate_started', {
            parentInvocationId: 'inv_root',
            childInvocationId: 'inv-child',
        }));
        state.eventListener?.(event(2, 'agent_task_started', {
            parentInvocationId: 'inv_root',
            childInvocationId: 'inv-child',
        }));
        expect(reads.filter(input => input.afterSeq != null)).toHaveLength(0);
        await rs.runAllTimersAsync();
        expect(reads.filter(input => input.afterSeq != null)).toHaveLength(1);
        controller.dispose();
    } finally {
        rs.useRealTimers();
    }
});

test('SubAgent state uses the server invocation filter and receives matching live events', async () => {
    const reads: TimelineReadInput[] = [];
    const { controller, state } = activeHarness({
        readEvents: input => {
            reads.push(input);
            if (input.invocationId) {
                return Promise.resolve({
                    events: [event(2, 'workspace_file_written', {
                        invocationId: 'inv-child',
                        path: 'child.txt',
                    })],
                });
            }
            return Promise.resolve({ events: [], timelineProjection: subAgentProjection() });
        },
    });
    await controller.init();
    controller.openSubAgent('inv-child');
    await flushMicrotasks();
    expect(reads.at(-1)?.invocationId).toBe('inv-child');
    expect(controller.getSnapshot().subAgent.displayItems).toHaveLength(1);
    expect(controller.getSnapshot().subAgent.task?.displayName).toBe('critic');

    state.eventListener?.(event(3, 'workspace_file_written', {
        invocationId: 'inv-child',
        path: 'live-child.txt',
    }));
    expect(controller.getSnapshot().subAgent.displayItems.map(item => item.seq)).toEqual([2, 3]);
    controller.closeSubAgent();
    expect(controller.getSnapshot().subAgent.open).toBe(false);
    controller.dispose();
});

test('retry is typed, history is read-only, and resize persistence follows completion semantics', async () => {
    const failure = event(1, 'run_failed', {
        code: 'agent.failed',
        message: 'failed',
        technicalMessage: 'failed',
        retryable: true,
        userRetryable: true,
    });
    const { controller, state } = activeHarness();
    await controller.init();
    state.eventListener?.(failure);
    expect(controller.getSnapshot().detailsOpen).toBe(true);
    await flushMicrotasks();
    const retry = controller.getSnapshot().detail.sections.flatMap(section => section.actions ?? [])[0];
    expect(retry?.kind).toBe('retry');
    if (retry) controller.invokeDetailAction(retry);
    expect(state.retries).toBe(1);

    controller.startResize(500, 300, { min: 132, max: 600 });
    controller.moveResize(450);
    controller.finishResize(false);
    expect(state.patches).toHaveLength(0);
    controller.startResize(500, 300, { min: 132, max: 600 });
    controller.finishResize(true);
    expect(state.patches).toHaveLength(1);
    expect(controller.resizeByKey('ArrowUp', 300, { min: 132, max: 600 })).toBe(true);
    expect(state.patches).toHaveLength(2);
    controller.resetPanelHeight();
    expect(state.patches.at(-1)).toEqual({ runTimelineHeightPx: null });
    controller.dispose();

    const history = createRunTimelineController({
        mode: 'history',
        rootId: 'history-1',
        run: { runId: 'run-1', generationType: 'normal' },
        requestClose: () => undefined,
        deps: {
            readEvents: () => Promise.resolve({ events: [failure], timelineProjection: emptyProjection() }),
            reportError: () => undefined,
            tr,
        },
    });
    await history.init();
    history.openDetails();
    await flushMicrotasks();
    expect(history.getSnapshot().detail.sections.flatMap(section => section.actions ?? [])).toEqual([]);
    history.dispose();
});
