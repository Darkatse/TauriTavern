/**
 * Agent: a foreground run streams the reply it writes into the chat, then, after many rounds of reads, patches and a
 * SubAgent, streams its reasoning into the open run timeline.
 */
export default async function agent({ page, measure }) {
    // Agent mode shows the run timeline, collapsed.
    await page.evaluate(async () => {
        const settings = await import('/scripts/tauritavern/agent/agent-system-settings.js');
        await settings.saveAgentSystemSettings({ ...await settings.loadAgentSystemSettings(), agentModeEnabled: true });
    });
    const timeline = page.locator('#ttas_agent_run_timeline');
    const toggle = timeline.locator('.ttas-run-actions button[aria-expanded]');
    await toggle.waitFor();
    // A send runs the Agent inside a generation, with the stop button in place of the send button.
    await page.evaluate(async () => {
        const { startAndWaitForAgentRun } = await import('/scripts/tauritavern/agent/agent-run-controller.js');
        (await import('/script.js')).deactivateSendButtons();
        globalThis.__perfAgentRun = startAndWaitForAgentRun({
            generationType: 'normal',
            stableChatId: 'review-chat',
            promptSnapshot: {},
            options: { presentation: 'foreground', stream: true },
        });
    });
    await page.waitForFunction(() => globalThis.__perfAgent.subscribers() > 0);

    // Each update is its own input, about a paragraph of text.
    const push = update => () => page.evaluate(value => globalThis.__perfAgent.push(value), update);
    const chunks = text => text.match(/[\s\S]{1,320}/g);
    const words = text => text.trim().split(/\s+/).length;
    const line = i => `The keeper turns to page ${i} of the log and reads out the ships that passed the reef. `;
    const reply = Array.from({ length: 20 }, (_, i) => i % 2 ? `"${line(i).repeat(2)}"` : `*He nods.* ${line(i).repeat(2)}`)
        .join('\n\n');
    const write = {
        toolId: 'builtin:workspace.write_file', invocationId: 'inv_root', invocationExitPolicy: 'run_finish_allowed',
        toolCallIndex: 0, path: 'output/main.md', content: '', contentWords: 0,
    };
    await measure('agent/stream-chat-write',
        push({ type: 'replace', call: write }),
        ...chunks(reply).map(text => push({
            type: 'append', invocationId: 'inv_root', toolCallIndex: 0, field: 'content', text, wordDelta: words(text),
        })),
    );
    const renderedChatText = await page.locator('#chat .mes:last-child .mes_text').textContent();
    if (!renderedChatText?.includes('page 19 of the log')) {
        throw new Error('Expected streamed Agent reply to render into the active chat message');
    }
    // Round 1's write_file call ends before the next round starts, so the live projection holds only the newest round.
    await page.evaluate(() => globalThis.__perfAgent.push({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 0 }));
    // The later rounds reach the journal while the timeline is collapsed.
    await page.evaluate(({ events, projection }) => {
        globalThis.__perfAgent.project(projection);
        for (const { type, payload } of events) globalThis.__perfAgent.event(type, payload);
    }, runHistory(HISTORY_ROUNDS));

    // The timeline reads the live projection while it shows its event list.
    await toggle.click();
    const scroller = timeline.locator('.ttas-run-event-scroll');
    await scroller.waitFor();
    await timeline.locator('.ttas-subagent-tray').waitFor();
    if (await scroller.locator('.ttas-run-event').count() < 10) {
        throw new Error('Expected the run timeline to show the run history');
    }
    const reasoning = Array.from({ length: 6 }, (_, i) => line(i).repeat(3)).join('\n\n');
    const response = {
        invocationId: 'inv_root', invocationExitPolicy: 'run_finish_allowed', round: HISTORY_ROUNDS + 1, attempt: 1,
        text: '', reasoning: '', toolIds: [],
    };
    await measure('agent/stream-timeline-reasoning',
        push({ type: 'responseReplace', response }),
        ...chunks(reasoning).map(text => push({ type: 'responseAppend', invocationId: 'inv_root', text: '', reasoning: text, toolIds: [] })),
    );
    const renderedReasoning = await scroller.locator('.ttas-run-event-live-stream').last().textContent();
    if (!renderedReasoning?.includes('page 5 of the log')) {
        throw new Error('Expected streamed Agent reasoning to render into the run timeline');
    }
    if (await scroller.evaluate(element => element.scrollHeight - element.clientHeight - element.scrollTop >= 1)) {
        throw new Error('Expected the run timeline to follow its live card to the bottom');
    }

    // The run ends and saves its reply.
    await page.evaluate(async () => {
        globalThis.__perfAgent.event('run_completed');
        await globalThis.__perfAgentRun;
        (await import('/script.js')).activateSendButtons();
    });
}

const HISTORY_ROUNDS = 24;
const READS = ['lore/harbor.md', 'lore/keeper.md', 'notes/ships.md', 'notes/weather.md'];

/**
 * A long run's journal up to its newest round: round 1 writes the reply; each later round narrates every other time,
 * then reads, searches or patches; round 8 delegates to a SubAgent whose own calls belong to its invocation.
 */
function runHistory(rounds) {
    const events = [];
    const add = (type, payload) => events.push({ type, payload });
    const tool = (invocationId, round, index, name, path) => {
        const call = { round, invocationId, callId: `call_${invocationId}_${round}_${index}`, toolId: `builtin:${name}`, snapshotId: 'snapshot_perf', name };
        add('tool_call_requested', { ...call, argumentsRef: `tool-arguments/${invocationId}/${round}-${index}.json` });
        const size = { chars: 3600 + round * 40, words: 640 + round * 7 };
        if (name === 'workspace.write_file') add('workspace_file_written', { invocationId, path, mode: 'create', ...size, sha256: `${round}`.padStart(64, 'a') });
        if (name === 'workspace.apply_patch') {
            add('workspace_patch_applied', { invocationId, path, ...size, oldSha256: `${round - 1}`.padStart(64, 'a'), sha256: `${round}`.padStart(64, 'a'), replacements: 1 });
        }
        add('tool_call_completed', { ...call, isError: false, errorCode: null, message: null, elapsedMs: 30 + index * 10, resourceRefs: path ? [path] : [] });
    };
    const narrate = (invocationId, round, text) => add('model_completed', {
        invocationId, round, hasAssistantText: true, assistantTextChars: text.length, assistantTextWords: text.split(' ').length,
        hasReasoning: true, reasoningChars: 1800, reasoningWords: 320, narration: { source: 'assistantText', text, totalChars: text.length },
    });
    const task = { taskId: 'task_perf_scout', parentInvocationId: 'inv_root', childInvocationId: 'inv_scout', targetProfileId: 'lore-scout', workspaceKey: 'scout' };

    narrate('inv_root', 1, "Drafting the keeper's reply about the ships that passed the reef.");
    tool('inv_root', 1, 0, 'workspace.write_file', 'output/main.md');
    for (let round = 2; round <= rounds; round++) {
        if (round % 2 === 0) narrate('inv_root', round, `Checking the log against page ${round} before the next revision.`);
        if (round === 8) {
            tool('inv_root', round, 0, 'agent.delegate');
            add('agent_delegate_started', task);
            add('agent_task_started', { ...task, status: 'running' });
            narrate('inv_scout', 1, 'Looking for every ship the harbor notes mention.');
            tool('inv_scout', 1, 0, 'workspace.search_files');
            tool('inv_scout', 2, 0, 'workspace.read_file', READS[2]);
            add('agent_task_completed', { ...task, status: 'completed' });
        } else if (round % 3 === 2) {
            tool('inv_root', round, 0, 'workspace.read_file', READS[round % 4]);
            tool('inv_root', round, 1, 'workspace.read_file', READS[(round + 1) % 4]);
        } else if (round % 3 === 0) {
            tool('inv_root', round, 0, 'workspace.search_files');
            tool('inv_root', round, 1, 'workspace.read_file', READS[round % 4]);
        } else {
            tool('inv_root', round, 0, 'workspace.apply_patch', 'output/main.md');
        }
    }

    const at = '2026-01-01T00:00:00Z';
    const invocation = { parentInvocationId: '', status: 'running', createdAt: at, updatedAt: at };
    return {
        events,
        projection: {
            foregroundInvocationIds: ['inv_root'],
            invocations: [
                { ...invocation, invocationId: 'inv_root', profileId: 'default-writer', kind: 'root', exitPolicy: 'run_finish_allowed' },
                {
                    ...invocation, invocationId: 'inv_scout', parentInvocationId: 'inv_root', profileId: 'lore-scout', kind: 'subagent',
                    status: 'completed', exitPolicy: 'task_return_required',
                },
            ],
            delegationEdges: [{
                taskId: task.taskId, sourceInvocationId: 'inv_root', targetInvocationId: 'inv_scout', targetProfileId: 'lore-scout',
                workspaceKey: 'scout', continuation: 'return_to_parent', status: 'completed', resultRef: '', error: '', createdAt: at, updatedAt: at,
            }],
        },
    };
}
