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

export function createAgentPromptSnapshot(payload, metadata) {
    if (!Array.isArray(payload?.messages)) {
        throw new Error('agent.prompt_snapshot_messages_required: prompt assembly must produce messages');
    }
    if (payload.tools?.length || Object.hasOwn(payload, 'tool_choice')) {
        throw new Error('agent.external_tools_unsupported: Agent runtime owns the tool registry and choice');
    }
    const { messages, prompt: _prompt, ...generationParameters } = payload;
    return { ...metadata, messages: messages.map(toAgentModelMessage), generationParameters };
}

/** Provider-shaped projection for PromptManager token counting. */
export function agentMessageProjection(message) {
    const content = [];
    const calls = [];
    const native = {};
    const reasoning = [];
    let result = null;
    for (const part of message.parts) {
        switch (part.type) {
            case 'text': content.push({ type: 'text', text: part.text }); break;
            case 'media': content.push(structuredClone(part.value)); break;
            case 'resourceRef': content.push({ type: 'text', text: part.uri }); break;
            case 'reasoning': if (part.text) reasoning.push(part.text); break;
            case 'native':
                if (part.provider === 'openai.content_part') content.push(structuredClone(part.value));
                else native[part.provider] = structuredClone(part.value);
                break;
            case 'toolCall': {
                const call = part.call;
                const alias = call.providerMetadata?.modelAlias;
                if (!alias) throw new Error('agent.tool_history_alias_missing: historical call has no modelAlias');
                calls.push({ id: call.callId, type: 'function', function: {
                    name: alias,
                    arguments: JSON.stringify(typeof call.arguments === 'object' && call.arguments !== null ? call.arguments : {}),
                } });
                break;
            }
            case 'toolResult': result = part.result; break;
            default: throw new Error(`agent.invalid_model_message: unsupported content part ${part.type}`);
        }
    }
    const metadata = message.providerMetadata ?? {};
    return {
        role: message.role,
        content: result ? result.content : content.every(part => part.type === 'text')
            ? content.map(part => part.text).join('') : content,
        ...(metadata.openai?.name ? { name: metadata.openai.name } : {}),
        ...(metadata.promptComponent ? { _tauritavern_prompt_component: metadata.promptComponent } : {}),
        ...(metadata.message?.reasoning ? { reasoning: metadata.message.reasoning } : {}),
        ...(metadata.message?.reasoning_details ? { reasoning_details: metadata.message.reasoning_details } : {}),
        ...(metadata.message?.signature ? { signature: metadata.message.signature } : {}),
        ...(calls.length ? { tool_calls: calls } : {}),
        ...(result ? { tool_call_id: result.callId } : {}),
        ...(reasoning.length ? { reasoning_content: reasoning.join('\n\n') } : {}),
        ...(Object.keys(native).length ? { native } : {}),
    };
}

/** Latest-first, atomic protocol groups consumed by the existing history budget loop. */
export function prepareAgentHistory(messages, policy) {
    if (!Array.isArray(messages) || messages.length === 0 || messages.at(-1).role !== 'user') {
        throw new Error('agent.session_input_invalid: Agent history must end with the current user message');
    }
    const groups = [];
    for (let index = 0; index < messages.length;) {
        const message = toAgentModelMessage(messages[index++]);
        if (message.role === 'tool') {
            throw new Error('agent.tool_history_orphan: tool result has no preceding call');
        }
        const group = [message];
        const calls = message.parts.filter(part => part.type === 'toolCall').map(part => part.call);
        const pending = new Set(calls.map(call => call.callId));
        if (pending.size !== calls.length) throw new Error('agent.tool_history_duplicate: repeated call id');
        while (index < messages.length && messages[index].role === 'tool') {
            const reply = toAgentModelMessage(messages[index++]);
            const results = reply.parts.filter(part => part.type === 'toolResult');
            if (results.length !== 1 || !pending.delete(results[0].result.callId)) {
                throw new Error('agent.tool_history_orphan: tool result does not match an unanswered call');
            }
            group.push(reply);
        }
        // A stopped run can have confirmed results and unanswered calls. Keep its
        // record intact; only the next prompt uses a protocol-safe explanation.
        const agentMessages = pending.size ? [interruptedToolGroup(group)] : group;
        groups.push({ role: message.role, content: '', agentMessages, sourceCount: group.length });
    }
    const current = groups.pop();
    current.required = true;
    const limit = policy?.initialChatHistoryMessages ?? -1;
    const history = limit < 0 ? groups : limit === 0 ? [] : groups.slice(-limit);
    return [...history, current].reverse();
}

function interruptedToolGroup(messages) {
    const results = new Map(messages.flatMap(message => message.parts
        .filter(part => part.type === 'toolResult')
        .map(part => [part.result.callId, part.result])));
    const lines = ['The previous turn stopped. Actions without recorded results have an unknown outcome; check the current state before repeating them.'];
    for (const message of messages) {
        for (const part of message.parts) {
            if (part.type === 'text') lines.push(part.text);
            if (part.type === 'toolCall') {
                const alias = part.call.providerMetadata?.modelAlias;
                if (!alias) throw new Error('agent.tool_history_alias_missing: historical call has no modelAlias');
                const result = results.get(part.call.callId);
                const outcome = result
                    ? `Recorded ${result.isError ? 'error' : 'result'}: ${result.content}`
                    : 'Outcome unknown: no recorded result.';
                lines.push(`${alias}(${JSON.stringify(part.call.arguments)})\n${outcome}`);
            }
        }
    }
    return { role: 'assistant', parts: [{ type: 'text', text: lines.join('\n\n') }], providerMetadata: {} };
}
