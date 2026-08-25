import { act, waitFor } from '@testing-library/react';
import { expect, test } from '@rstest/core';

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

function restoreProperty(target: object, key: PropertyKey, descriptor?: PropertyDescriptor): void {
    if (descriptor) Object.defineProperty(target, key, descriptor);
    else Reflect.deleteProperty(target, key);
}

test('history timelines support multiple roots and clean every dialog path', async () => {
    const hostDescriptor = Object.getOwnPropertyDescriptor(window, '__TAURITAVERN__');
    const showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal');
    const closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: {
            api: {
                agent: {
                    readEvents: () => Promise.resolve({
                        events: [],
                        timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
                    }),
                },
            },
        },
    });
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

    try {
        act(() => {
            openAgentRunTimelineDialog(run('run-1'));
            openAgentRunTimelineDialog(run('run-2'));
        });
        await waitFor(() => expect(document.querySelectorAll('dialog.ttas-run-history-dialog')).toHaveLength(2));
        const roots = [...document.querySelectorAll<HTMLElement>('.ttas-run-history-dialog .ttas-run-panel')];
        expect(new Set(roots.map(root => root.id)).size).toBe(2);

        const dialogs = [...document.querySelectorAll<HTMLDialogElement>('dialog.ttas-run-history-dialog')];
        act(() => dialogs[0]?.close());
        expect(document.querySelectorAll('dialog.ttas-run-history-dialog')).toHaveLength(1);

        const remaining = document.querySelector<HTMLDialogElement>('dialog.ttas-run-history-dialog');
        const cancel = new Event('cancel', { cancelable: true });
        act(() => { remaining?.dispatchEvent(cancel); });
        expect(cancel.defaultPrevented).toBe(true);
        expect(document.querySelector('dialog.ttas-run-history-dialog')).toBeNull();

        Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
            configurable: true,
            value() { throw new Error('showModal failed'); },
        });
        expect(() => act(() => openAgentRunTimelineDialog(run('run-3')))).toThrow('showModal failed');
        expect(document.querySelector('dialog.ttas-run-history-dialog')).toBeNull();
    } finally {
        document.querySelectorAll<HTMLDialogElement>('dialog.ttas-run-history-dialog').forEach(dialog => dialog.close());
        restoreProperty(window, '__TAURITAVERN__', hostDescriptor);
        restoreProperty(HTMLDialogElement.prototype, 'showModal', showModalDescriptor);
        restoreProperty(HTMLDialogElement.prototype, 'close', closeDescriptor);
    }
});
