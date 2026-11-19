/**
 * Agent: a foreground run streams the reply it writes into the chat, then streams its reasoning into the open run
 * timeline.
 */
export default async function agent({ page, measure }) {
    // Agent mode shows the run timeline, collapsed.
    await page.evaluate(async () => {
        const settings = await import('/scripts/tauritavern/agent/agent-system-settings.js');
        await settings.saveAgentSystemSettings({ ...await settings.loadAgentSystemSettings(), agentModeEnabled: true });
    });
    const toggle = page.locator('#ttas_agent_run_timeline .ttas-run-actions button[aria-expanded]');
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
    // End round 1's write_file call before round 2 reasoning starts so the live projection reflects round 2 only.
    await page.evaluate(() => globalThis.__perfAgent.push({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 0 }));

    // The timeline reads the live projection while it shows its event list.
    await toggle.click();
    await page.locator('#ttas_agent_run_timeline .ttas-run-event-scroll').waitFor();
    const reasoning = Array.from({ length: 6 }, (_, i) => line(i).repeat(3)).join('\n\n');
    const response = {
        invocationId: 'inv_root', invocationExitPolicy: 'run_finish_allowed', round: 2, attempt: 1, text: '', reasoning: '',
        toolIds: [],
    };
    await measure('agent/stream-timeline-reasoning',
        push({ type: 'responseReplace', response }),
        ...chunks(reasoning).map(text => push({ type: 'responseAppend', invocationId: 'inv_root', text: '', reasoning: text, toolIds: [] })),
    );
    const renderedReasoning = await page.locator('#ttas_agent_run_timeline .ttas-run-event-live-stream').last().textContent();
    if (!renderedReasoning?.includes('page 5 of the log')) {
        throw new Error('Expected streamed Agent reasoning to render into the run timeline');
    }

    // The run ends and saves its reply.
    await page.evaluate(async () => {
        globalThis.__perfAgent.event('run_completed');
        await globalThis.__perfAgentRun;
        (await import('/script.js')).activateSendButtons();
    });
}
