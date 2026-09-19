import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import type { AgentSystemSettings } from './settings-store';
import { RunTimelineApp } from './RunTimelineApp';
import { createRunTimelineController } from './RunTimelineController';
import type { ActiveTimelineOptions, RunTimelineController } from './RunTimelineContract';
import type { AgentSystemTr } from './i18n';

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
        retryPresentation: async () => {},
        resumeRun: () => Promise.resolve(),
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

test('detail navigation scrolls through older pages with bounded buttons and lazy detail reads', async () => {
    const reads: number[] = [];
    const detailReads: string[] = [];
    const translate: AgentSystemTr = (key, params) => params?.path ? `${key}:${params.path}` : key;
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: { api: { agent: { readWorkspaceFile: ({ path }: { path: string }) => {
            detailReads.push(path);
            return Promise.resolve({ path, text: `content:${path}`, chars: 5, words: 1, sha256: 'hash' });
        } } } },
    });
    const controller = createRunTimelineController({
        mode: 'history',
        rootId: 'detail-history',
        run: { runId: 'run-1' },
        requestClose: () => undefined,
        deps: {
            readEvents: ({ beforeSeq }) => {
                reads.push(beforeSeq ?? 0);
                const last = beforeSeq === Number.MAX_SAFE_INTEGER ? 720 : (beforeSeq ?? 1) - 1;
                return Promise.resolve({
                    events: Array.from({ length: 240 }, (_, index) => fileEvent(last - 239 + index)),
                    timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
                });
            },
            reportError: error => { throw error; },
            tr: translate,
        },
    });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={translate} />);
    await act(() => controller.init());
    await userEvent.setup().click(screen.getByRole('button', { name: 'showTimelineDetails' }));
    const nav = screen.getByRole('navigation', { name: 'timelineDetails' });
    Object.defineProperties(nav, { clientWidth: { value: 340 }, clientHeight: { value: 64 } });
    fireEvent.scroll(nav);
    nav.scrollTop = 192;
    fireEvent.scroll(nav);
    const earlier = await within(nav).findByTitle('timelineEventFileWritten:file-501.txt');
    await userEvent.setup().click(earlier);
    await screen.findByText('content:file-501.txt');
    expect(nav.querySelectorAll('button').length).toBeLessThan(40);

    nav.scrollTop = 0;
    fireEvent.scroll(nav);
    await waitFor(() => expect(controller.getSnapshot().displayItems).toHaveLength(480));
    await within(nav).findByTitle('timelineEventFileWritten:file-481.txt');
    expect(controller.getSnapshot().selectedSeq).toBe(501);
    nav.scrollTop = 0;
    fireEvent.scroll(nav);
    await waitFor(() => expect(controller.getSnapshot().displayItems).toHaveLength(720));
    expect(detailReads).toEqual(['file-720.txt', 'file-501.txt']);
    nav.scrollTop = 0;
    fireEvent.scroll(nav);
    await userEvent.setup().click(await within(nav).findByTitle('timelineEventFileWritten:file-1.txt'));
    await screen.findByText('content:file-1.txt');
    expect(reads).toEqual([Number.MAX_SAFE_INTEGER, 481, 241]);
    expect(controller.getSnapshot().hasMoreBefore).toBe(false);
    expect(nav.querySelectorAll('button').length).toBeLessThan(40);
});

const taskDetail: TauriTavernAgentTaskDetail = {
    runId: 'run-1',
    taskId: 'task-1',
    parentInvocationId: 'inv_root',
    childInvocationId: 'inv-child',
    targetProfileId: 'critic',
    workspaceKey: 'critic',
    status: 'completed',
    continuation: 'return_to_parent',
    task: {
        title: 'Review character motivation',
        objective: 'Find the missing motivation.',
        context: 'Only review the final scene.',
        customConstraint: 'Do not change the narrator.',
    },
    resultRef: 'agent-results/inv-child.json',
    result: null,
    error: null,
};

async function renderTaskTimeline(
    event: TauriTavernAgentRunEvent,
    task: TauriTavernAgentTaskDetail = taskDetail,
    projection: TauriTavernAgentRunTimelineProjection = {
        foregroundInvocationIds: ['inv_root'], invocations: [], delegationEdges: [],
    },
) {
    const reads: Parameters<TauriTavernAgentApi['readTaskDetail']>[0][] = [];
    const readTaskDetail: TauriTavernAgentApi['readTaskDetail'] = input => {
        reads.push(input);
        return Promise.resolve(task);
    };
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: { api: { agent: { readTaskDetail } } },
    });
    const controller = createRunTimelineController({
        mode: 'history', rootId: 'task-details', run: { runId: task.runId }, requestClose: () => undefined,
        deps: {
            readEvents: () => Promise.resolve({ events: [event], timelineProjection: projection }),
            reportError: error => { throw error; }, tr,
        },
    });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());
    expect(reads).toEqual([]);
    return { controller, reads };
}

test('delegation details lazily show the objective and disclose the original task context', async () => {
    const { reads } = await renderTaskTimeline({
        ...fileEvent(300), type: 'agent_delegate_started',
        payload: { taskId: 'task-1', parentInvocationId: 'inv_root', childInvocationId: 'inv-child' },
    });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'showTimelineDetails' }));
    const objective = await screen.findByText('Find the missing motivation.');
    expect(objective.closest('details')?.open).toBe(true);
    expect(reads).toEqual([{ runId: 'run-1', taskId: 'task-1', includeResult: false }]);
    expect(screen.getByText(/task-1/).closest('details')?.open).toBe(false);
    const supplement = screen.getByText(/Do not change the narrator\./).closest('details');
    if (!supplement) throw new Error('expected supplementary detail block');
    expect(supplement.open).toBe(false);
    await user.click(within(supplement).getByText('timelineTaskBrief'));
    expect(supplement.open).toBe(true);
});

test('returned task details keep the summary and questions visible while folding supporting results', async () => {
    const { controller, reads } = await renderTaskTimeline({
        ...fileEvent(300), type: 'task_return_completed',
        payload: { taskId: 'task-1', parentInvocationId: 'inv_root', childInvocationId: 'inv-child' },
    }, {
        ...taskDetail,
        result: {
            summary: 'Add a reason for her return.', summaryRef: 'summaries/inv-child.md',
            output: {
                summary: 'Add a reason for her return.', status: 'completed',
                warnings: ['The previous scene is unavailable.'],
                questionsForCaller: ['Can the letter be mentioned?'],
                findings: [{ observation: 'The letter explains her choice.' }],
                customFinding: 'Retain the quiet tone.',
            },
        },
    });
    act(() => controller.openSubAgent('inv-child'));
    const summary = await screen.findByText('Add a reason for her return.');
    expect(summary.closest('details')?.open).toBe(true);
    expect(reads).toEqual([{ runId: 'run-1', taskId: 'task-1', includeResult: true }]);
    expect(screen.getByText(/The previous scene is unavailable\./).closest('details')?.open).toBe(true);
    expect(screen.getByText(/Can the letter be mentioned\?/).closest('details')?.open).toBe(true);
    expect(screen.getByText(/Retain the quiet tone\./).closest('details')?.open).toBe(false);
    expect(screen.getByText(/Do not change the narrator\./).closest('details')?.open).toBe(false);
    expect(screen.getAllByText(/Review character motivation/)).toHaveLength(1);
});

test('projected handoff details read the brief even when the original request is outside the journal page', async () => {
    const task: TauriTavernAgentTaskDetail = { ...taskDetail, continuation: 'transfer_control', resultRef: null };
    const { controller, reads } = await renderTaskTimeline({
        ...fileEvent(300), type: 'run_completed', payload: { invocationId: 'inv-child' },
    }, task, {
        foregroundInvocationIds: ['inv_root', 'inv-child'], invocations: [],
        delegationEdges: [{
            taskId: task.taskId, sourceInvocationId: task.parentInvocationId,
            targetInvocationId: task.childInvocationId, targetProfileId: task.targetProfileId,
            workspaceKey: task.workspaceKey, continuation: task.continuation, status: task.status,
            createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z',
        }],
    });
    const boundary = controller.getSnapshot().displayItems.find(item => item.kind === 'handoff');
    if (!boundary) throw new Error('expected projected handoff boundary');
    act(() => controller.selectItem(boundary.seq));
    await userEvent.setup().click(screen.getByRole('button', { name: 'showTimelineDetails' }));
    const objective = await screen.findByText('Find the missing motivation.');
    expect(objective.closest('details')?.open).toBe(true);
    expect(reads).toEqual([{ runId: 'run-1', taskId: 'task-1', includeResult: false }]);
    expect(screen.getByText('timelineHandoffBrief').closest('details')?.open).toBe(false);
});

test('active timeline renders a streaming write card with tail and metric', async () => {
    let liveHandler: ((update: TauriTavernAgentRunLiveUpdate) => void) | null = null;
    const deps: ActiveTimelineOptions['deps'] = {
        readEvents: () => Promise.resolve({
            events: [fileEvent(1)],
            timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
        }),
        reportError: error => { throw error; },
        tr,
        loadSettings: () => Promise.resolve(settings()),
        patchSettings: (current, patch) => Promise.resolve({ ...current, ...patch }),
        subscribeSettings: () => () => undefined,
        getActiveRun: () => ({ runId: 'run-1', generationType: 'normal' }),
        subscribeRunState: () => () => undefined,
        subscribeRunEvents: () => () => undefined,
        subscribeLiveProjection: (_runId, handler) => {
            liveHandler = handler;
            return () => undefined;
        },
        scheduleFrame: callback => callback(),
        retryFailure: () => Promise.resolve(),
        retryPresentation: async () => {},
        resumeRun: () => Promise.resolve(),
    };
    const controller = createRunTimelineController({ mode: 'active', deps });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());
    await userEvent.setup().click(screen.getByRole('button', { name: 'expandTimeline' }));

    act(() => {
        liveHandler?.({
            type: 'replace',
            call: {
                toolId: 'builtin:workspace.write_file',
                invocationId: 'inv_root',
                invocationExitPolicy: 'run_finish_allowed',
                toolCallIndex: 0,
                path: 'reply.md',
                content: '',
                contentWords: 0,
            },
        });
        liveHandler?.({
            type: 'append',
            invocationId: 'inv_root',
            toolCallIndex: 0,
            field: 'content',
            text: 'a streamed tail line',
            wordDelta: 4,
        });
    });

    const card = document.querySelector('.ttas-run-event.is-live');
    expect(card).not.toBeNull();
    expect(card?.getAttribute('aria-live')).toBe('off');
    expect(document.querySelector('.ttas-run-heading-copy small')?.getAttribute('aria-live')).toBe('off');
    expect(card?.getAttribute('style')).toContain('116px');
    expect(card?.querySelector('.ttas-run-event-live-stream')?.textContent).toBe('a streamed tail line');
    expect(card?.textContent).toContain('timelineLiveWriting');
    expect(card?.querySelector('.ttas-run-event-live-metric')?.textContent).toBe('+timelineWordCount');
    const disclosure = card?.querySelector('summary');
    if (!disclosure) throw new Error('expected live disclosure');
    expect(disclosure.getAttribute('aria-expanded')).toBe('false');
    act(() => liveHandler?.({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0,
        field: 'content', text: '\nline 2\nline 3\nline 4', wordDelta: 6 }));
    expect(card?.textContent).not.toContain('a streamed tail line');
    const user = userEvent.setup();
    await user.click(disclosure);
    expect(disclosure.getAttribute('aria-expanded')).toBe('true');
    expect(card?.querySelector('details')?.open).toBe(true);
    expect(card?.querySelector('.ttas-run-event-live-stream')?.textContent).toBe('a streamed tail line\nline 2\nline 3\nline 4');
    act(() => liveHandler?.({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0,
        field: 'content', text: '\nline 5', wordDelta: 2 }));
    expect(card?.querySelector('.ttas-run-event-live-stream')?.textContent).toContain('line 4\nline 5');
    await user.click(disclosure);
    expect(disclosure.getAttribute('aria-expanded')).toBe('false');
    expect(card?.textContent).not.toContain('a streamed tail line');
    act(() => {
        liveHandler?.({ type: 'reasoningReplace', reasoning: {
            invocationId: 'inv_root', invocationExitPolicy: 'run_finish_allowed', text: 'Planning', toolIds: [],
        } });
        liveHandler?.({ type: 'reasoningAppend', toolIds: [], invocationId: 'inv_root', text: ' the edit' });
    });
    const thinking = document.querySelector('.ttas-run-event-live.is-reasoning');
    expect(thinking?.querySelector('.ttas-run-event-live-stream')?.textContent).toBe('Planning the edit');
    expect(thinking?.closest('.ttas-run-event')?.textContent).toContain('timelineLiveReasoning');
    expect(thinking?.closest('.ttas-run-event')?.querySelector('.ttas-run-event-live-metric')).toBeNull();
    act(() => {
        liveHandler?.({ type: 'reasoningAppend', invocationId: 'inv_root', text: '', toolIds: ['builtin:workspace.write_file'] });
    });
    const toolNames = thinking?.closest('.ttas-run-event')?.querySelector('.ttas-run-event-tool-names');
    expect(toolNames?.textContent).toBe('writing a file');
    expect(toolNames?.getAttribute('title')).toBe('writing a file');
    expect(toolNames?.parentElement?.textContent).toContain('writing a filetimelineLiveReasoning');
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
            retryPresentation: async () => {},
            resumeRun: () => Promise.resolve(),
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
