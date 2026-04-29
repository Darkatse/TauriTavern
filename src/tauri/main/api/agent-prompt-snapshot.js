// @ts-check

const LEGACY_DRY_RUN_SOURCE = 'legacy-generate-dry-run';

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any> }} input
 * @returns {Promise<{ promptSnapshot: { chatCompletionPayload: any; worldInfoActivation?: any }; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshot(input = {}) {
    const generationType = normalizeGenerationType(input.generationType);
    const generateOptions = normalizeGenerateOptions(input.generateOptions);
    const script = await import('../../../script.js');

    if (script.main_api !== 'openai') {
        throw new Error('agent.phase2b_chat_completion_required: Agent Phase 2B requires the OpenAI/chat-completion frontend path');
    }

    const { generateData, worldInfoActivation } = await captureAgentDryRun(script, generationType, {
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
            ...(worldInfoActivation ? { worldInfoActivation } : {}),
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

async function captureAgentDryRun(script, generationType, generateOptions) {
    let generateData = null;
    let worldInfoActivation = null;
    const generateListener = (capturedGenerateData, dryRun) => {
        if (dryRun === true) {
            generateData = capturedGenerateData;
        }
    };
    const worldInfoListener = (payload) => {
        if (payload?.isDryRun === true && payload?.isFinal === true) {
            worldInfoActivation = normalizeWorldInfoActivationBatch(payload);
        }
    };

    script.eventSource.on(script.event_types.GENERATE_AFTER_DATA, generateListener);
    script.eventSource.on(script.event_types.WORLDINFO_SCAN_DONE, worldInfoListener);
    try {
        await script.Generate(generationType, generateOptions, true);
    } finally {
        script.eventSource.removeListener(script.event_types.GENERATE_AFTER_DATA, generateListener);
        script.eventSource.removeListener(script.event_types.WORLDINFO_SCAN_DONE, worldInfoListener);
    }

    if (!generateData || typeof generateData !== 'object' || Array.isArray(generateData)) {
        throw new Error('agent.prompt_snapshot_missing: dryRun did not emit generate_after_data');
    }

    return { generateData, worldInfoActivation };
}

function normalizeWorldInfoActivationBatch(payload) {
    const entries = Array.from(payload?.activated?.entries?.values?.() ?? []).map(normalizeWorldInfoEntry);
    return {
        timestampMs: Date.now(),
        trigger: String(payload?.trigger || 'normal').trim() || 'normal',
        entries,
    };
}

function normalizeWorldInfoEntry(entry) {
    const position = normalizeWorldInfoPosition(entry?.position);
    return {
        world: String(entry?.world || '').trim(),
        uid: typeof entry?.uid === 'number' ? entry.uid : String(entry?.uid ?? '').trim(),
        displayName: normalizeWorldInfoDisplayName(entry),
        constant: Boolean(entry?.constant),
        content: String(entry?.content || ''),
        ...(position ? { position } : {}),
    };
}

function normalizeWorldInfoPosition(position) {
    switch (Number(position)) {
        case 0:
            return 'before';
        case 1:
            return 'after';
        case 2:
            return 'an_top';
        case 3:
            return 'an_bottom';
        case 4:
            return 'depth';
        case 5:
            return 'em_top';
        case 6:
            return 'em_bottom';
        case 7:
            return 'outlet';
        default:
            return undefined;
    }
}

function normalizeWorldInfoDisplayName(entry) {
    const comment = String(entry?.comment || '').trim();
    if (comment) {
        return comment;
    }

    if (Array.isArray(entry?.key)) {
        const key = entry.key.find((value) => String(value || '').trim());
        if (key !== undefined) {
            return String(key).trim();
        }
    }

    return String(entry?.uid ?? '').trim();
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
