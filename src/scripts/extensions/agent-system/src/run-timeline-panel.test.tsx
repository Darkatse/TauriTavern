import { act, waitFor } from '@testing-library/react';
import { expect, rs, test } from '@rstest/core';

import { openAgentRunTimelineDialog } from './run-timeline-panel';

function run(runId: string): TauriTavernAgentRunSummary {
    return {
        runId,
        workspaceId: `workspace-${runId}`,
        stableChatId: `chat-${runId}`,
        chatRef: { kind: 'character', characterId: 'character', fileName: 'character.jsonl' },
        generationType: 'normal',
        presentation: 'foreground',
        status: 'completed',
        createdAt: '2026-01-01T00:00:00Z',
        updatedAt: '2026-01-01T00:00:01Z',
        commitCount: 0,
    };
}

test('history timelines support multiple roots and clean every dialog path', async () => {
    const readEvents: TauriTavernAgentApi['readEvents'] = ({ runId }) => Promise.resolve({
        events: [{
            seq: 1,
            id: `event-${runId}`,
            runId,
            timestamp: '2026-01-01T00:00:00Z',
            level: 'info',
            type: 'workspace_file_written',
            payload: { path: `${runId}.txt`, chars: 5, words: 1 },
        }],
        timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
    });
    rs.stubGlobal('__TAURITAVERN__', {
        api: {
            agent: {
                profiles: {},
                readEvents,
            },
        },
    });
    act(() => {
        openAgentRunTimelineDialog(run('run-1'));
        openAgentRunTimelineDialog(run('run-2'));
    });
    await waitFor(() => expect(document.querySelectorAll('dialog.ttas-run-history-dialog')).toHaveLength(2));
    const roots = [...document.querySelectorAll<HTMLElement>('.ttas-run-history-dialog .ttas-run-panel')];
    expect(new Set(roots.map(root => root.id)).size).toBe(2);
    await waitFor(() => {
        expect(roots[0]?.textContent).toContain('run-1.txt');
        expect(roots[1]?.textContent).toContain('run-2.txt');
    });

    const dialogs = [...document.querySelectorAll<HTMLDialogElement>('dialog.ttas-run-history-dialog')];
    act(() => dialogs[0]?.close());
    await waitFor(() => expect(document.querySelectorAll('dialog.ttas-run-history-dialog')).toHaveLength(1));

    const remaining = document.querySelector<HTMLDialogElement>('dialog.ttas-run-history-dialog');
    const cancel = new Event('cancel', { cancelable: true });
    act(() => { remaining?.dispatchEvent(cancel); });
    expect(cancel.defaultPrevented).toBe(true);
    await waitFor(() => expect(document.querySelector('dialog.ttas-run-history-dialog')).toBeNull());

    rs.spyOn(HTMLDialogElement.prototype, 'showModal').mockImplementation(() => {
        throw new Error('showModal failed');
    });
    expect(() => act(() => openAgentRunTimelineDialog(run('run-3')))).toThrow('showModal failed');
    expect(document.querySelector('dialog.ttas-run-history-dialog')).toBeNull();
});
