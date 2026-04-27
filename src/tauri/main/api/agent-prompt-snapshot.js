// @ts-check

const LEGACY_DRY_RUN_SOURCE = 'legacy-generate-dry-run';

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any> }} input
 * @returns {Promise<{ promptSnapshot: { chatCompletionPayload: any }; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshot(input = {}) {
    const generationType = normalizeGenerationType(input.generationType);
    const generateOptions = normalizeGenerateOptions(input.generateOptions);
    const script = await import('../../../script.js');

    if (script.main_api !== 'openai') {
        throw new Error('agent.phase2b_chat_completion_required: Agent Phase 2B requires the OpenAI/chat-completion frontend path');
    }

    const generateData = await captureGenerateAfterData(script, generationType, {
        ...generateOptions,
        agentMode: true,
    });
    const messages = generateData?.prompt;
    assertMessagesReady(messages);
    assertNoExternalToolTurns(messages);

    const openai = await import('../../../scripts/openai.js');
    const model = openai.getChatCompletionModel(openai.oai_settings);
    if (!model) {
        throw new Error('agent.model_required: current chat-completion source did not resolve a model');
    }

    const { generate_data: payload } = await openai.createGenerationParameters(
        openai.oai_settings,
        model,
        generationType,
        structuredClone(messages),
        {
            jsonSchema: generateOptions.jsonSchema ?? null,
            agentMode: true,
        },
    );

    assertNoExternalTools(payload);
    assertNoExternalToolTurns(payload.messages);

    return {
        promptSnapshot: {
            chatCompletionPayload: payload,
        },
        generationIntent: {
            source: LEGACY_DRY_RUN_SOURCE,
            generationType,
            chatCompletionSource: payload.chat_completion_source,
            model: payload.model,
        },
    };
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function normalizeGenerateOptions(value) {
    if (value == null) {
        return {};
    }
    if (typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('agent.generate_options_invalid: generateOptions must be an object');
    }
    return value;
}

async function captureGenerateAfterData(script, generationType, generateOptions) {
    let captured = null;
    const listener = (generateData, dryRun) => {
        if (dryRun === true) {
            captured = generateData;
        }
    };

    script.eventSource.on(script.event_types.GENERATE_AFTER_DATA, listener);
    try {
        await script.Generate(generationType, generateOptions, true);
    } finally {
        script.eventSource.removeListener(script.event_types.GENERATE_AFTER_DATA, listener);
    }

    if (!captured || typeof captured !== 'object' || Array.isArray(captured)) {
        throw new Error('agent.prompt_snapshot_missing: dryRun did not emit generate_after_data');
    }

    return captured;
}

function assertMessagesReady(messages) {
    if (!Array.isArray(messages)) {
        throw new Error('agent.prompt_snapshot_messages_required: dryRun did not produce chat-completion messages');
    }
}

function assertNoExternalTools(payload) {
    const tools = payload?.tools;
    if (Array.isArray(tools) && tools.length > 0) {
        throw new Error('agent.external_tools_unsupported_phase2b: Agent Phase 2B owns the tool registry');
    }
    if (Object.prototype.hasOwnProperty.call(payload || {}, 'tool_choice')) {
        throw new Error('agent.external_tool_choice_unsupported_phase2b: Agent Phase 2B owns tool choice');
    }
}

function assertNoExternalToolTurns(messages) {
    if (!Array.isArray(messages)) {
        return;
    }

    const hasToolTurn = messages.some((message) => {
        const role = String(message?.role || '').toLowerCase();
        return role === 'tool'
            || (Array.isArray(message?.tool_calls) && message.tool_calls.length > 0);
    });

    if (hasToolTurn) {
        throw new Error('agent.external_tool_turns_unsupported_phase2b: prompt snapshot already contains tool turns');
    }
}
