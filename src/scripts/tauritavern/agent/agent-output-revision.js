export async function reviseAgentOutput(guidance, abortController, runtime = null) {
    const text = String(guidance ?? '').trim();
    if (!text) throw new Error('agent.guidance_empty: describe the change you want to make');
    const script = runtime || await import('/script.js' /* webpackIgnore: true */);
    const message = script.chat.at(-1);
    const runId = message?.extra?.tauritavern?.agent?.runId;
    if (!runId || message.is_user || message.is_system) {
        throw new Error('agent.revision_unavailable: select an Agent reply at the end of the chat');
    }
    const agent = window.__TAURITAVERN__?.api?.agent;
    if (!agent) throw new Error('TauriTavern Agent API is unavailable');
    const checkpoint = await agent.readCheckpoint(runId);
    return script.resumeAgentRunInChat({
        runId,
        generationType: checkpoint.run.generationType,
        checkpoint,
        revisionGuidance: text,
        abortController,
    });
}
