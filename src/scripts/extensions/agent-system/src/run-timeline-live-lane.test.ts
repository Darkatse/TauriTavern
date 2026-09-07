import { expect, test } from '@rstest/core';

import { createRunTimelineLiveLane, type RunTimelineLiveLane } from './run-timeline-live-lane';

type LiveHandler = (update: TauriTavernAgentRunLiveUpdate) => void;

function harness(overrides: Partial<Parameters<typeof createRunTimelineLiveLane>[0]> = {}) {
    const state = {
        handler: null as LiveHandler | null,
        subscribedRunId: '',
        unsubscribed: 0,
        changes: 0,
        errors: [] as unknown[],
    };
    const lane = createRunTimelineLiveLane({
        subscribeLiveProjection: (runId, handler) => {
            state.subscribedRunId = runId;
            state.handler = handler;
            return () => {
                state.unsubscribed += 1;
            };
        },
        scheduleFrame: callback => callback(),
        onChange: () => {
            state.changes += 1;
        },
        onError: error => {
            state.errors.push(error);
        },
        ...overrides,
    });
    return { lane, state };
}

function writeCall(
    content: string,
    overrides: Record<string, unknown> = {},
): TauriTavernAgentRunLiveToolCall {
    return {
        toolId: 'builtin:workspace.write_file',
        invocationId: 'inv_root',
        invocationExitPolicy: 'run_finish_allowed',
        toolCallIndex: 0,
        path: 'reply.md',
        content,
        contentWords: 0,
        ...overrides,
    };
}

function patchCall(overrides: Record<string, unknown> = {}): TauriTavernAgentRunLiveToolCall {
    return {
        toolId: 'builtin:workspace.apply_patch',
        invocationId: 'inv_root',
        invocationExitPolicy: 'run_finish_allowed',
        toolCallIndex: 1,
        path: 'notes.md',
        oldString: '',
        oldStringWords: 0,
        newString: '',
        newStringWords: 0,
        ...overrides,
    };
}

function streamingWrite(lane: RunTimelineLiveLane, state: { handler: LiveHandler | null }): LiveHandler {
    lane.attach('run-1');
    const handler = state.handler;
    if (!handler) throw new Error('expected the lane to subscribe');
    handler({ type: 'snapshot', calls: [], reasoning: [] });
    handler({ type: 'replace', call: writeCall('') });
    return handler;
}

test('streams a foreground write call into a double-height card with a line-based tail', () => {
    const { lane, state } = harness();
    const handler = streamingWrite(lane, state);
    handler({
        type: 'append',
        invocationId: 'inv_root',
        toolCallIndex: 0,
        field: 'content',
        text: 'l1\nl2\n你好\nworld',
        wordDelta: 5,
    });

    const items = lane.items();
    expect(items).toHaveLength(1);
    const item = items[0];
    if (!item) throw new Error('expected the live item');
    expect(item.rowSpan).toBe(2);
    expect(item.kind).toBe('write');
    expect(item.titleKey).toBe('timelineLiveWriting');
    expect(item.titleParams).toEqual({ path: 'reply.md' });
    expect(item.live?.tail).toBe('…l2\n你好\nworld');
    expect(item.live?.truncated).toBe(true);
    expect(item.live?.streamTone).toBe('neutral');
    expect(item.live).toMatchObject({ addedWords: 5, removedWords: 0 });
    expect(state.changes).toBeGreaterThan(0);
});

test('publishes at most once per scheduled frame regardless of update volume', () => {
    const scheduled: Array<() => void> = [];
    const { lane, state } = harness({
        scheduleFrame: callback => {
            scheduled.push(callback);
        },
    });
    const handler = streamingWrite(lane, state);
    handler({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text: 'a', wordDelta: 1 });
    handler({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text: 'b', wordDelta: 1 });
    expect(scheduled).toHaveLength(1);
    expect(state.changes).toBe(0);
    scheduled.shift()?.();
    expect(state.changes).toBe(1);
    expect(lane.items()[0]?.live?.tail).toBe('ab');
    expect(lane.items()[0]?.live).toMatchObject({ addedWords: 2 });
});

test('keeps a bounded rolling tail across large snapshots and deltas', () => {
    const { lane, state } = harness();
    lane.attach('run-1');
    const handler = state.handler;
    if (!handler) throw new Error('expected the lane to subscribe');
    handler({ type: 'snapshot', reasoning: [], calls: [writeCall('x'.repeat(500), { contentWords: 1 })] });
    handler({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text: 'y', wordDelta: 1 });

    expect(lane.items()[0]?.live).toMatchObject({
        tail: `…${'x'.repeat(419)}y`,
        truncated: true,
        addedWords: 2,
    });
});

test('ignores SubAgent updates without publishing and replaces retry generations', () => {
    const { lane, state } = harness();
    lane.attach('run-1');
    const handler = state.handler;
    if (!handler) throw new Error('expected the lane to subscribe');
    handler({ type: 'snapshot', calls: [], reasoning: [] });
    handler({ type: 'replace', call: writeCall('hidden', { invocationExitPolicy: 'task_return_required' }) });
    expect(lane.items()).toHaveLength(0);

    handler({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text: 'stale', wordDelta: 1 });
    expect(lane.items()).toHaveLength(0);
    expect(state.changes).toBe(0);

    handler({ type: 'replace', call: writeCall('first') });
    handler({ type: 'replace', call: writeCall('retry') });
    expect(lane.items()[0]?.live?.tail).toBe('retry');
});

test('remove immediately deletes only the addressed live call', () => {
    const { lane, state } = harness();
    lane.attach('run-1');
    const handler = state.handler;
    if (!handler) throw new Error('expected the lane to subscribe');
    handler({
        type: 'snapshot',
        reasoning: [],
        calls: [
            writeCall('first', { toolCallIndex: 0 }),
            writeCall('second', { toolCallIndex: 1 }),
        ],
    });
    handler({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 0 });
    const remaining = lane.items();
    expect(remaining).toHaveLength(1);
    expect(remaining[0]?.live?.tail).toBe('second');

    handler({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 1 });
    expect(lane.items()).toHaveLength(0);
});

test('detach and dispose unsubscribe and drop all cards idempotently', () => {
    const { lane, state } = harness();
    const handler = streamingWrite(lane, state);
    handler({ type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text: 'body', wordDelta: 1 });
    expect(lane.items()).toHaveLength(1);

    lane.detach();
    expect(state.unsubscribed).toBe(1);
    expect(lane.items()).toHaveLength(0);

    lane.attach('run-2');
    expect(state.subscribedRunId).toBe('run-2');
    expect(state.unsubscribed).toBe(1);

    lane.dispose();
    lane.dispose();
    expect(state.unsubscribed).toBe(2);
    expect(lane.items()).toHaveLength(0);
});

test('patch cards follow the arriving field: red while locating, green while writing', () => {
    const { lane, state } = harness();
    lane.attach('run-1');
    const handler = state.handler;
    if (!handler) throw new Error('expected the lane to subscribe');
    handler({ type: 'snapshot', reasoning: [], calls: [patchCall()] });

    handler({
        type: 'append',
        invocationId: 'inv_root',
        toolCallIndex: 1,
        field: 'oldString',
        text: 'old line',
        wordDelta: 2,
    });
    let item = lane.items()[0];
    expect(item?.kind).toBe('patch');
    expect(item?.titleKey).toBe('timelineLivePatching');
    expect(item?.live?.streamTone).toBe('removed');
    expect(item?.live?.tail).toBe('old line');

    handler({
        type: 'append',
        invocationId: 'inv_root',
        toolCallIndex: 1,
        field: 'newString',
        text: 'new text',
        wordDelta: 2,
    });
    item = lane.items()[0];
    expect(item?.live?.streamTone).toBe('added');
    expect(item?.live?.tail).toBe('new text');
    expect(item?.live).toMatchObject({ addedWords: 2, removedWords: 2 });
});

test('reasoning shares the live lane, stays bounded, and replaces retries independently of tools', () => {
    const { lane, state } = harness();
    const handler = streamingWrite(lane, state);
    const reasoning: TauriTavernAgentRunLiveReasoning = {
        invocationId: 'inv_root', invocationExitPolicy: 'run_finish_allowed', text: 'l1\nl2\nl3', toolIds: [],
    };
    handler({ type: 'snapshot', calls: [writeCall('body')], reasoning: [reasoning] });
    handler({ type: 'reasoningAppend', toolIds: [], invocationId: 'inv_root', text: '\n思考' });
    expect(lane.items()[0]?.live).toEqual({ tail: '…l2\nl3\n思考', truncated: true, streamTone: 'reasoning', toolLabel: '' });
    handler({ type: 'reasoningAppend', invocationId: 'inv_root', text: '', toolIds: ['builtin:workspace.read_file'] });
    expect(lane.items()[0]?.live).toMatchObject({ toolLabel: 'reading a file', tail: '…l2\nl3\n思考' });
    handler({ type: 'reasoningAppend', invocationId: 'inv_root', text: '', toolIds: ['mcp/a:search', 'mcp/b:search'] });
    expect(lane.items()[0]?.live).toMatchObject({ toolLabel: 'reading a file · search [mcp/a:search] · search [mcp/b:search]' });
    handler({ type: 'reasoningReplace', reasoning: { ...reasoning, invocationId: 'child', invocationExitPolicy: 'task_return_required' } });
    handler({ type: 'reasoningAppend', toolIds: [], invocationId: 'child', text: 'hidden' });
    expect(lane.items()).toHaveLength(2);
    handler({ type: 'reasoningReplace', reasoning: { ...reasoning, text: 'retry' } });
    expect(lane.items()[0]?.live).toMatchObject({ tail: 'retry', toolLabel: '' });
    handler({ type: 'reasoningRemove', invocationId: 'inv_root' });
    expect(lane.items().map(item => item.live?.tail)).toEqual(['body']);
    handler({ type: 'reasoningReplace', reasoning });
    handler({ type: 'snapshot', calls: [], reasoning: [] });
    expect(lane.items()).toEqual([]);
});
