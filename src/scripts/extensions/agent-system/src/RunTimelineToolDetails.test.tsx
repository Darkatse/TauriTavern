import { act, cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { RunTimelineApp } from './RunTimelineApp';
import { createRunTimelineController } from './RunTimelineController';
import type { RunTimelineController } from './RunTimelineContract';

const tr = (key: string): string => key;
const controllers: RunTimelineController[] = [];

afterEach(() => {
    cleanup();
    controllers.splice(0).forEach(controller => controller.dispose());
    Reflect.deleteProperty(window, '__TAURITAVERN__');
});

function event(seq: number, type: string, payload: Record<string, unknown>): TauriTavernAgentRunEvent {
    return { seq, id: `event-${seq}`, runId: 'run-1', timestamp: '2026-01-01T00:00:00Z', level: 'info', type, payload };
}

test('completed shell shows the command and opens both input and output', async () => {
    const command = "printf 'hello\\n' > draft.md\ncat draft.md";
    const argumentsRef = 'tool-args/inv_root/round-001-shell.json';
    const resultPath = 'tool-results/inv_root/round-001-shell.json';
    const output = 'hello';
    const call = { invocationId: 'inv_root', round: 1, callId: 'shell', toolId: 'builtin:workspace.shell', name: 'workspace.shell' };
    const files: Record<string, unknown> = {
        [argumentsRef]: { command },
        [resultPath]: { toolId: call.toolId, content: output, isError: false },
    };
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: { api: { agent: { readWorkspaceFile: ({ path }: { path: string }) => {
            if (!(path in files)) throw new Error(`Unexpected detail file: ${path}`);
            return Promise.resolve({ path, text: JSON.stringify(files[path]), chars: 0, words: 0, sha256: 'hash' });
        } } } },
    });
    const controller = createRunTimelineController({
        mode: 'history', rootId: 'shell-history', run: { runId: 'run-1' }, requestClose: () => undefined,
        deps: {
            readEvents: () => Promise.resolve({
                events: [
                    event(1, 'tool_call_requested', { ...call, command, argumentsRef }),
                    event(2, 'tool_result_stored', { ...call, path: resultPath }),
                    event(3, 'tool_call_completed', { ...call, elapsedMs: 12 }),
                ],
                timelineProjection: { foregroundInvocationIds: ['inv_root'], invocations: [], delegationEdges: [] },
            }),
            reportError: error => { throw error; },
            tr,
        },
    });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());

    const row = screen.getByRole('button', { name: /printf/ });
    expect(row.textContent).toContain(command);
    await userEvent.setup().click(screen.getByRole('button', { name: 'showTimelineDetails' }));
    expect((await screen.findByText(command, { selector: 'pre', normalizer: value => value })).textContent).toBe(command);
    await screen.findByText(output, { selector: 'pre' });
});
