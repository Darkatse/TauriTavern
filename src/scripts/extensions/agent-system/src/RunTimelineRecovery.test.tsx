import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { RunTimelineApp } from './RunTimelineApp';
import { createRunTimelineController } from './RunTimelineController';
import type { ActiveTimelineOptions, RunTimelineController } from './RunTimelineContract';

const tr = (key: string): string => key;
const controllers: RunTimelineController[] = [];
afterEach(() => {
    cleanup();
    controllers.splice(0).forEach(controller => controller.dispose());
});

test('presentation save failures remain visible and can be retried without restarting the run', async () => {
    let runState: Parameters<ActiveTimelineOptions['deps']['subscribeRunState']>[0] | undefined;
    let finishSave: (() => void) | undefined;
    const retried: string[] = [];
    const resumed: string[] = [];
    const controller = createRunTimelineController({
        mode: 'active',
        deps: {
            readEvents: () => Promise.resolve({ events: [],
                timelineProjection: { foregroundInvocationIds: [], invocations: [], delegationEdges: [] },
            }),
            reportError: error => { throw error; },
            tr,
            loadSettings: () => Promise.resolve({
                agentModeEnabled: true, chatInputToggleHidden: false,
                activeProfileId: 'default-writer', editingProfileId: 'default-writer',
                activeTab: 'profiles', runTimelineHeightPx: null,
            }),
            patchSettings: (current, patch) => Promise.resolve({ ...current, ...patch }),
            subscribeSettings: () => () => undefined,
            getActiveRun: () => ({ runId: 'run-1' }),
            subscribeRunState: listener => { runState = listener; return () => undefined; },
            subscribeRunEvents: () => () => undefined,
            retryFailure: () => Promise.resolve(),
            resumeRun: runId => { resumed.push(runId); return Promise.resolve(); },
            retryPresentation: runId => {
                retried.push(runId);
                return new Promise(resolve => { finishSave = resolve; });
            },
        },
    });
    controllers.push(controller);
    render(<RunTimelineApp controller={controller} tr={tr} />);
    await act(() => controller.init());
    act(() => runState?.({
        activeRun: null,
        lastEvent: { id: 'event-2', runId: 'run-1', seq: 2, timestamp: '2026-01-01T00:00:00Z',
            level: 'info', type: 'run_cancelled', payload: null },
        presentationError: 'disk full',
    }));
    expect(screen.getByRole('alert').textContent).toContain('disk full');
    const retry = screen.getByRole('button', { name: 'timelineRetryPresentation' });
    await userEvent.setup().click(retry);
    expect(retried).toEqual(['run-1']);
    expect(resumed).toEqual([]);
    expect(retry.hasAttribute('disabled')).toBe(true);
    act(() => finishSave?.());
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    await userEvent.setup().click(await screen.findByRole('button', { name: 'timelineActionResume' }));
    expect(resumed).toEqual(['run-1']);
});
