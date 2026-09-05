import { expect, test } from '@rstest/core';

import {
    buildEventDetailTargets,
    hasModelTurnNarration,
    isDisplayableRunEvent,
    presentRunEvent,
    timelineItemsFromEvents,
} from './run-event-presenter';
import {
    projectSubAgentTasks,
    RETURN_TO_PARENT_CONTINUATION,
    TRANSFER_CONTROL_CONTINUATION,
} from './run-invocation-projector';
import type { TimelineProjection } from './RunTimelineContract';

function event(
    seq: number,
    type: string,
    payload: Record<string, unknown> = {},
): TauriTavernAgentRunEvent {
    return {
        seq,
        id: `event-${seq}`,
        runId: 'run-1',
        timestamp: `2026-06-07T00:00:0${seq}.000Z`,
        level: type.includes('failed') ? 'error' : 'info',
        type,
        payload,
    };
}

test('SubAgent projection does not flatten child lifecycle into the foreground chain', () => {
    const projection: TimelineProjection = {
        foregroundInvocationIds: ['inv_root'],
        invocations: [
            {
                invocationId: 'inv_root',
                parentInvocationId: '',
                profileId: 'writer',
                kind: 'root',
                status: 'running',
                exitPolicy: 'run_finish_allowed',
                createdAt: '2026-06-07T00:00:00.000Z',
                updatedAt: '2026-06-07T00:00:00.000Z',
            },
            {
                invocationId: 'inv-child',
                parentInvocationId: 'inv_root',
                profileId: 'scene-critic',
                kind: 'subagent',
                status: 'completed',
                exitPolicy: 'task_return_required',
                createdAt: '2026-06-07T00:00:01.000Z',
                updatedAt: '2026-06-07T00:00:05.000Z',
            },
        ],
        delegationEdges: [{
            taskId: 'task-1',
            sourceInvocationId: 'inv_root',
            targetInvocationId: 'inv-child',
            targetProfileId: 'scene-critic',
            workspaceKey: 'scene-critic',
            continuation: RETURN_TO_PARENT_CONTINUATION,
            status: 'completed',
            resultRef: 'agent-results/inv-child.json',
            error: '',
            createdAt: '2026-06-07T00:00:01.000Z',
            updatedAt: '2026-06-07T00:00:05.000Z',
        }],
    };
    const events = [
        event(1, 'agent_delegate_started', {
            taskId: 'task-1',
            parentInvocationId: 'inv_root',
            childInvocationId: 'inv-child',
            targetProfileId: 'scene-critic',
            workspaceKey: 'scene-critic',
            eventScope: { invocationId: 'inv_root', relatedInvocationIds: ['inv-child'] },
        }),
        event(2, 'agent_task_started', {
            taskId: 'task-1',
            parentInvocationId: 'inv_root',
            childInvocationId: 'inv-child',
            targetProfileId: 'scene-critic',
            status: 'running',
            eventScope: { invocationId: 'inv_root', relatedInvocationIds: ['inv-child'] },
        }),
        event(3, 'model_completed', {
            invocationId: 'inv-child',
            round: 1,
            hasReasoning: true,
        }),
        event(4, 'tool_call_completed', {
            invocationId: 'inv-child',
            callId: 'call-return',
            toolId: 'builtin:task.return',
            name: 'task.return',
        }),
        event(5, 'task_return_completed', {
            taskId: 'task-1',
            parentInvocationId: 'inv_root',
            childInvocationId: 'inv-child',
            status: 'completed',
            resultRef: 'agent-results/inv-child.json',
            eventScope: { invocationId: 'inv-child', relatedInvocationIds: ['inv_root'] },
        }),
    ];

    expect(projectSubAgentTasks(projection)).toMatchObject([{
        displayName: 'scene-critic',
        status: 'completed',
    }]);
    expect(timelineItemsFromEvents(events, {
        foregroundInvocationIds: projection.foregroundInvocationIds,
        delegationEdges: projection.delegationEdges,
    }).map(item => item.type)).toEqual(['agent_delegate_started']);
    expect(timelineItemsFromEvents(events, { invocationId: 'inv-child' }).map(item => item.type)).toEqual([
        'agent_delegate_started',
        'agent_task_started',
        'task_return_completed',
    ]);
});

test('handoff projection keeps one foreground boundary and its typed detail target', () => {
    const projection: TimelineProjection = {
        foregroundInvocationIds: ['inv_root', 'inv-editor'],
        invocations: [],
        delegationEdges: [{
            taskId: 'handoff-1',
            sourceInvocationId: 'inv_root',
            targetInvocationId: 'inv-editor',
            targetProfileId: 'line-editor',
            workspaceKey: 'line-editor',
            continuation: TRANSFER_CONTROL_CONTINUATION,
            status: 'completed',
            resultRef: '',
            error: '',
            createdAt: '2026-06-07T00:00:02.000Z',
            updatedAt: '2026-06-07T00:00:02.000Z',
        }],
    };
    const events = [
        event(1, 'agent_handoff_accepted', {
            taskId: 'handoff-1',
            sourceInvocationId: 'inv_root',
            newInvocationId: 'inv-editor',
            targetProfileId: 'line-editor',
            workspaceKey: 'line-editor',
            eventScope: { invocationId: 'inv_root', relatedInvocationIds: ['inv-editor'] },
        }),
        event(2, 'agent_invocation_started', {
            invocationId: 'inv-editor',
            parentInvocationId: 'inv_root',
            profileId: 'line-editor',
            kind: 'handoff',
            status: 'running',
        }),
        event(3, 'tool_call_completed', {
            invocationId: 'inv-editor',
            callId: 'call-read',
            toolId: 'builtin:workspace.read_file',
            name: 'workspace.read_file',
            displayMetrics: { chars: 80, words: 12 },
        }),
        event(4, 'workspace_patch_applied', {
            invocationId: 'inv-editor',
            path: 'output/main.md',
            chars: 120,
            words: 18,
            replacements: 1,
        }),
        event(5, 'run_completed'),
    ];

    expect(projectSubAgentTasks(projection)).toEqual([]);
    const items = timelineItemsFromEvents(events, {
        foregroundInvocationIds: projection.foregroundInvocationIds,
        delegationEdges: projection.delegationEdges,
    });
    expect(items.map(item => item.type)).toEqual([
        'agent_handoff_accepted',
        'tool_call_completed',
        'workspace_patch_applied',
        'run_completed',
    ]);
    expect(items[0]).toMatchObject({
        kind: 'handoff',
        titleKey: 'timelineEventHandoffAccepted',
        titleParams: { agent: 'line-editor' },
    });
    const handoffItem = items[0];
    if (!handoffItem) throw new Error('expected a handoff item');
    expect(buildEventDetailTargets(handoffItem, events)).toEqual([{
        type: 'handoff',
        labelKey: 'timelineHandoff',
        taskId: 'handoff-1',
        sourceInvocationId: 'inv_root',
        newInvocationId: 'inv-editor',
        targetProfileId: 'line-editor',
        workspaceKey: 'line-editor',
        status: 'accepted',
    }]);
});

test('model turns stay hidden while associated reasoning remains lazily addressable', () => {
    const modelEvent = event(4, 'model_completed', {
        round: 2,
        modelResponsePath: 'model-responses/round-002.json',
        toolCallCount: 1,
        hasReasoning: true,
        reasoningChars: 30,
        reasoningWords: 5,
    });
    const toolEvent = event(5, 'tool_call_completed', {
        round: 2,
        callId: 'call-1',
        toolId: 'builtin:workspace.read_file',
        name: 'workspace.read_file',
    });

    expect(isDisplayableRunEvent(modelEvent)).toBe(false);
    expect(hasModelTurnNarration(modelEvent)).toBe(false);
    expect(timelineItemsFromEvents([modelEvent])).toEqual([]);
    expect(buildEventDetailTargets(presentRunEvent(toolEvent), [modelEvent, toolEvent])).toEqual([
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 2 },
    ]);
});
