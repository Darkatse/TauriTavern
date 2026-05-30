// @ts-check

import {
    loadAgentContextPolicy,
    normalizeAgentContextPolicy,
} from '../../../scripts/tauritavern/agent/agent-context-policy.js';
import {
    loadResolvedAgentSystemPrompt,
    normalizeAgentSystemPrompt,
} from '../../../scripts/tauritavern/agent/agent-system-prompt.js';
import { normalizeFrozenRunInputSnapshot } from '../../../scripts/tauritavern/agent/frozen-run-input-snapshot.js';

const LEGACY_DRY_RUN_SOURCE = 'legacy-generate-dry-run';

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any>; profileId?: string; agentContextPolicy?: Record<string, any>; agentSystemPrompt?: string }} input
 * @returns {Promise<{ currentPromptSnapshotSeed: any; frozenRunInputSnapshot: any; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshotSeed(input = {}) {
    const generationType = normalizeGenerationType(input.generationType);
    const generateOptions = normalizeGenerateOptions(input.generateOptions);
    const agentContextPolicy = input.agentContextPolicy
        ? normalizeAgentContextPolicy(input.agentContextPolicy)
        : await loadAgentContextPolicy(input.profileId);
    const agentSystemPrompt = Object.prototype.hasOwnProperty.call(input, 'agentSystemPrompt')
        ? normalizeAgentSystemPrompt(input.agentSystemPrompt)
        : await loadResolvedAgentSystemPrompt(input.profileId);
    const script = await import('../../../script.js');

    if (script.main_api !== 'openai') {
        throw new Error('agent.phase2b_chat_completion_required: Agent Phase 2B requires the OpenAI/chat-completion frontend path');
    }

    const { generateData } = await captureAgentDryRun(script, generationType, {
        ...generateOptions,
        agentMode: true,
        agentContextPolicy,
        agentSystemPrompt,
    });
    const messages = generateData?.prompt;
    assertMessagesReady(messages);
    assertNoExternalToolTurns(messages);
    const frozenRunInputSnapshot = normalizeFrozenRunInputSnapshot(generateData.frozenRunInputSnapshot);

    return {
        currentPromptSnapshotSeed: {
            generationType,
            contextPolicy: agentContextPolicy,
            messages: structuredClone(messages),
            jsonSchema: generateOptions.jsonSchema ?? null,
            worldInfoActivation: frozenRunInputSnapshot.worldInfoActivation ?? null,
        },
        frozenRunInputSnapshot,
        generationIntent: {
            source: LEGACY_DRY_RUN_SOURCE,
            generationType,
        },
    };
}

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any>; profileId?: string; agentContextPolicy?: Record<string, any>; agentSystemPrompt?: string }} input
 * @returns {Promise<{ promptSnapshot: { contextPolicy: any; chatCompletionPayload: any; worldInfoActivation?: any }; frozenRunInputSnapshot: any; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshot(input = {}) {
    return materializeCurrentPromptSnapshot(await buildAgentPromptSnapshotSeed(input));
}

export async function materializeCurrentPromptSnapshot(input) {
    const seed = input?.currentPromptSnapshotSeed;
    if (!seed || typeof seed !== 'object' || Array.isArray(seed)) {
        throw new Error('agent.current_prompt_snapshot_seed_required: currentPromptSnapshotSeed must be an object');
    }
    const generationType = normalizeGenerationType(seed.generationType);
    const messages = seed.messages;
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
            jsonSchema: seed.jsonSchema ?? null,
            agentMode: true,
        },
    );

    assertNoExternalTools(payload);
    assertNoExternalToolTurns(payload.messages);

    return {
        promptSnapshot: {
            contextPolicy: seed.contextPolicy,
            chatCompletionPayload: payload,
            ...(seed.worldInfoActivation ? { worldInfoActivation: seed.worldInfoActivation } : {}),
        },
        frozenRunInputSnapshot: input.frozenRunInputSnapshot,
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
    const generateListener = (capturedGenerateData, dryRun) => {
        if (dryRun === true) {
            generateData = capturedGenerateData;
        }
    };

    script.eventSource.on(script.event_types.GENERATE_AFTER_DATA, generateListener);
    try {
        await script.Generate(generationType, generateOptions, true);
    } finally {
        script.eventSource.removeListener(script.event_types.GENERATE_AFTER_DATA, generateListener);
    }

    if (!generateData || typeof generateData !== 'object' || Array.isArray(generateData)) {
        throw new Error('agent.prompt_snapshot_missing: dryRun did not emit generate_after_data');
    }

    return { generateData };
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
