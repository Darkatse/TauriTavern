const ROLES = new Set(['system', 'developer', 'user', 'assistant', 'tool']);

/** Keep the Agent transcript canonical; OpenAI-shaped messages are an input boundary. */
export function toAgentModelMessage(message) {
    if (!message || !ROLES.has(message.role)) {
        throw new Error('agent.invalid_model_message: message role is invalid');
    }
    if (Array.isArray(message.parts)) return structuredClone(message);
    if (message.role === 'tool' || message.tool_calls?.length) {
        throw new Error('agent.external_tool_turns_unsupported: tool history requires canonical Agent messages');
    }

    const content = message.content;
    const parts = content == null ? [] : (Array.isArray(content) ? content : [content]).map(part => {
        if (typeof part === 'string') return { type: 'text', text: part };
        if (part?.type === 'text') return { type: 'text', text: part.text };
        return { type: 'native', provider: 'openai.content_part', value: structuredClone(part) };
    });
    if (message.reasoning_content) {
        parts.push({ type: 'reasoning', text: message.reasoning_content, provider_metadata: {} });
    }
    for (const [provider, value] of Object.entries(message.native ?? {})) {
        parts.push({ type: 'native', provider, value: structuredClone(value) });
    }
    return {
        role: message.role,
        parts,
        providerMetadata: {
            ...(message.name ? { openai: { name: message.name } } : {}),
            ...(message._tauritavern_prompt_component ? { promptComponent: message._tauritavern_prompt_component } : {}),
            ...(message.reasoning || message.reasoning_details || message.signature ? {
                message: {
                    ...(message.reasoning ? { reasoning: message.reasoning } : {}),
                    ...(message.reasoning_details ? { reasoning_details: structuredClone(message.reasoning_details) } : {}),
                    ...(message.signature ? { signature: message.signature } : {}),
                },
            } : {}),
        },
    };
}
