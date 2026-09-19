export async function reviseOutput(guidance, abortController, runtime = null) {
    const text = String(guidance ?? '').trim();
    if (!text) throw new Error('agent.guidance_empty: describe the change you want to make');
    const script = runtime || await import('/script.js' /* webpackIgnore: true */);
    const message = script.chat.at(-1);
    if (!message?.mes || message.is_user || message.is_system || message.role === 'tool') {
        throw new Error('output_revision.unavailable: select an assistant reply at the end of the chat');
    }
    const runId = message.extra?.tauritavern?.agent?.runId;
    if (!runId) return script.reviseLegacyOutputInChat(text, abortController);
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
