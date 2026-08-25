import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { RunHistoryPanel } from './RunHistoryPanel';
import {
    createRunHistoryController,
    type RunHistoryController,
    type RunHistoryListInput,
} from './RunHistoryController';
import {
    createRunRetentionController,
    type RunRetentionController,
} from './RunRetentionController';

function formatParam(value: unknown): string {
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
        return String(value);
    }
    return JSON.stringify(value) ?? '';
}

const tr = (key: string, params: Record<string, unknown> = {}): string => (
    [key, ...Object.entries(params).map(([name, value]) => `${name}=${formatParam(value)}`)].join(' ')
);

function run(runId: string, characterId: string): TauriTavernAgentRunSummary {
    return {
        runId,
        workspaceId: `workspace-${runId}`,
        stableChatId: `stable-${runId}`,
        chatRef: { kind: 'character', characterId, fileName: `${characterId}.jsonl` },
        generationType: 'normal',
        profileId: 'default-writer',
        presentation: 'foreground',
        status: 'completed',
        createdAt: '2026-01-01T00:00:00Z',
        updatedAt: '2026-01-01T00:00:01Z',
        terminalAt: '2026-01-01T00:00:01Z',
        commitCount: 0,
    };
}

function retentionSettings(): TauriTavernAgentRunRetentionSettings {
    return {
        autoPruneEnabled: false,
        keepRecentTerminalRuns: 100,
        keepFullRecentRuns: 20,
    };
}

function prunePlan(hasWork: boolean): TauriTavernAgentRunPrunePlan {
    return {
        retention: { keepRecentTerminalRuns: 10, keepFullRecentRuns: 5 },
        detailLimit: 8,
        terminalRunCount: hasWork ? 11 : 10,
        nonTerminalRunCount: 0,
        blockedRunCount: 0,
        fullRetainedRunCount: 5,
        coreRetainedRunCount: 5,
        slimCandidateCount: hasWork ? 1 : 0,
        deleteCandidateCount: 0,
        totalSlimFileCount: hasWork ? 2 : 0,
        totalSlimByteCount: hasWork ? 1024 : 0,
        totalDeleteFileCount: 0,
        totalDeleteByteCount: 0,
        totalCandidateFileCount: hasWork ? 2 : 0,
        totalCandidateByteCount: hasWork ? 1024 : 0,
        candidateDetailsTruncated: false,
        candidates: [],
        blockedDetailsTruncated: false,
        blockedRuns: [],
    };
}

function unusedRetention(): RunRetentionController {
    return createRunRetentionController({
        getRetentionApi: () => ({
            readSettings: () => Promise.resolve(retentionSettings()),
            updateSettings: () => Promise.reject(new Error('unused')),
            planPrune: () => Promise.reject(new Error('unused')),
            applyPrune: () => Promise.reject(new Error('unused')),
        }),
        confirmAction: () => Promise.resolve(false),
        notifySuccess: () => undefined,
        notifyWarning: () => undefined,
        tr,
    });
}

const disposables: Array<RunHistoryController | RunRetentionController> = [];

afterEach(() => {
    cleanup();
    disposables.splice(0).forEach((controller) => controller.dispose());
});

test('the newest History filter response wins', async () => {
    const requests: Array<{
        input: RunHistoryListInput;
        resolve: (value: unknown) => void;
    }> = [];
    const history = createRunHistoryController({
        listRuns: (input) => new Promise((resolve) => {
            requests.push({ input, resolve });
        }),
        currentChatRunFilter: () => Promise.resolve({
            chatRef: { kind: 'character', characterId: 'Current', fileName: 'Current.jsonl' },
            stableChatId: 'stable-current',
        }),
        openRun: () => undefined,
    });
    const retention = unusedRetention();
    disposables.push(history, retention);
    const user = userEvent.setup();
    render(<RunHistoryPanel controller={history} retention={retention} tr={tr} />);

    await user.click(screen.getByRole('button', { name: 'runHistoryCurrentChat' }));
    await waitFor(() => expect(requests).toHaveLength(1));
    await user.click(screen.getByRole('button', { name: 'runHistoryAllChats' }));
    await waitFor(() => expect(requests).toHaveLength(2));

    expect(requests[0]?.input.stableChatId).toBe('stable-current');
    expect(requests[1]?.input.stableChatId).toBeUndefined();
    await act(async () => {
        requests[1]?.resolve({ runs: [run('new-all-run', 'Newest Chat')], nextCursor: null });
        await Promise.resolve();
    });
    expect(screen.getByText('Newest Chat')).toBeDefined();

    await act(async () => {
        requests[0]?.resolve({ runs: [run('stale-current-run', 'Stale Chat')], nextCursor: null });
        await Promise.resolve();
    });
    expect(screen.getByText('Newest Chat')).toBeDefined();
    expect(screen.queryByText('Stale Chat')).toBeNull();
});

test('Retention validates, confirms, applies, and refreshes History', async () => {
    const state = {
        readCalls: 0,
        planCalls: 0,
        confirmCalls: 0,
        applyCalls: 0,
        historyCalls: 0,
        confirmed: false,
        successes: [] as string[],
    };
    const history = createRunHistoryController({
        listRuns: () => {
            state.historyCalls += 1;
            return Promise.resolve({ runs: [], nextCursor: null });
        },
        currentChatRunFilter: () => Promise.reject(new Error('unused')),
        openRun: () => undefined,
    });
    const retention = createRunRetentionController({
        getRetentionApi: () => ({
            readSettings: () => {
                state.readCalls += 1;
                return Promise.resolve(retentionSettings());
            },
            updateSettings: () => Promise.reject(new Error('unused')),
            planPrune: () => {
                state.planCalls += 1;
                return Promise.resolve(prunePlan(true));
            },
            applyPrune: () => {
                state.applyCalls += 1;
                return Promise.resolve({
                    retention: { keepRecentTerminalRuns: 10, keepFullRecentRuns: 5 },
                    detailLimit: 8,
                    slimmedRunCount: 1,
                    deletedRunCount: 0,
                    failedRunCount: 0,
                    removedFileCount: 2,
                    removedByteCount: 1024,
                    failedDetailsTruncated: false,
                    failedRuns: [],
                    afterPlan: prunePlan(false),
                });
            },
        }),
        confirmAction: () => {
            state.confirmCalls += 1;
            return Promise.resolve(state.confirmed);
        },
        notifySuccess: (message) => {
            state.successes.push(message);
        },
        notifyWarning: () => undefined,
        tr,
    });
    disposables.push(history, retention);
    const user = userEvent.setup();
    render(<RunHistoryPanel controller={history} retention={retention} tr={tr} />);

    const retentionHeading = screen.getByRole('heading', { name: 'runRetention' });
    const retentionSection = retentionHeading.closest('section');
    if (!retentionSection) {
        throw new Error('expected the Retention section');
    }
    const panel = within(retentionSection);
    await user.click(panel.getByRole('button', { name: 'refresh' }));
    await waitFor(() => expect(state.readCalls).toBe(1));

    const keepHistory = panel.getByRole<HTMLInputElement>('spinbutton', { name: 'runRetentionKeepHistory' });
    const keepFull = panel.getByRole<HTMLInputElement>('spinbutton', { name: 'runRetentionKeepFull' });
    await user.clear(keepHistory);
    await user.type(keepHistory, '5');
    await user.clear(keepFull);
    await user.type(keepFull, '6');
    await user.click(panel.getByRole('button', { name: 'runRetentionAnalyze' }));
    await waitFor(() => expect(retention.getSnapshot().error).not.toBe(''));
    expect(state.planCalls).toBe(0);
    expect(retentionSection.textContent).toContain(retention.getSnapshot().error);

    await user.clear(keepHistory);
    await user.type(keepHistory, '10');
    await user.clear(keepFull);
    await user.type(keepFull, '5');
    await user.click(panel.getByRole('button', { name: 'runRetentionAnalyze' }));
    await waitFor(() => expect(state.planCalls).toBe(1));

    const apply = panel.getByRole('button', { name: 'runRetentionApply' });
    await user.click(apply);
    expect(state.confirmCalls).toBe(1);
    expect(state.applyCalls).toBe(0);

    state.confirmed = true;
    await user.click(apply);
    await waitFor(() => expect(state.applyCalls).toBe(1));
    await waitFor(() => expect(state.historyCalls).toBe(1));
    expect(state.successes).toEqual(['runRetentionApplied bytes=1.0 KB files=fileCount count=2 count=0']);
});
