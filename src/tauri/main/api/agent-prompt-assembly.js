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

const PROMPT_ASSEMBLY_SOURCE = 'frontend-prompt-assembly-broker';

/**
 * Builds an Agent PromptSnapshot through the real SillyTavern chat-completion
 * PromptManager pipeline, using frozen prompt inputs and preset settings.
 *
 * @param {Record<string, any>} input
 * @returns {Promise<{ promptSnapshot: { contextPolicy: any; chatCompletionPayload: any; worldInfoActivation?: any }; frozenRunInputSnapshot: any; generationIntent: any; assembly: any }>}
 */
export async function buildPromptAssemblySnapshot(input = {}) {
    const request = await normalizePromptAssemblyRequest(input);
    const openai = await import('../../../scripts/openai.js');
    if (request.modelId && !hasChatCompletionSource(request.settings)) {
        throw new Error('prompt_assembly.source_required: modelId overrides require settings.chat_completion_source');
    }
    const model = request.modelId || openai.getChatCompletionModel(request.settings);
    if (!model) {
        throw new Error('prompt_assembly.model_required: chat-completion settings did not resolve a model');
    }
    const settings = openai.normalizeChatCompletionSettingsForPromptAssembly(request.settings);

    const result = await openai.assembleOpenAIChatCompletionPrompt({
        settings,
        model,
        generationType: request.generationType,
        promptInputs: request.promptInputs,
        macroContext: request.macroContext,
        jsonSchema: request.jsonSchema,
        agentMode: true,
        agentSystemPrompt: request.agentSystemPrompt,
    });

    const payload = result.chatCompletionPayload;
    assertNoExternalTools(payload);
    assertNoExternalToolTurns(payload.messages);

    return {
        promptSnapshot: {
            contextPolicy: request.agentContextPolicy,
            chatCompletionPayload: payload,
            ...(request.worldInfoActivation ? { worldInfoActivation: request.worldInfoActivation } : {}),
        },
        frozenRunInputSnapshot: request.frozenRunInputSnapshot,
        generationIntent: {
            source: PROMPT_ASSEMBLY_SOURCE,
            generationType: request.generationType,
            chatCompletionSource: payload.chat_completion_source,
            model: payload.model,
        },
        assembly: {
            schemaVersion: 1,
            engine: 'sillytavern-chat-completion-prompt-manager',
            tokenCounts: result.tokenCounts,
        },
    };
}

async function normalizePromptAssemblyRequest(input) {
    if (!input || typeof input !== 'object' || Array.isArray(input)) {
        throw new Error('prompt_assembly.input_invalid: input must be an object');
    }

    const profileId = normalizeOptionalString(input.profileId ?? input.profile_id);
    const frozenRunInputSnapshot = normalizeRequiredFrozenRunInputSnapshot(input);
    const generationType = normalizeGenerationType(
        input.generationType ?? input.generation_type ?? frozenRunInputSnapshot?.generationType,
    );
    if (frozenRunInputSnapshot.generationType !== generationType) {
        throw new Error('prompt_assembly.generation_type_mismatch: generationType must match FrozenRunInputSnapshot.generationType');
    }
    const directPromptInputs = input.promptInputs ?? input.prompt_inputs;
    if (directPromptInputs != null) {
        throw new Error('prompt_assembly.prompt_inputs_duplicated: promptInputs must be read from FrozenRunInputSnapshot');
    }
    const directWorldInfoActivation = input.worldInfoActivation ?? input.world_info_activation;
    if (directWorldInfoActivation != null) {
        throw new Error('prompt_assembly.world_info_activation_duplicated: worldInfoActivation must be read from FrozenRunInputSnapshot');
    }
    const directMacroContext = input.macroContext ?? input.macro_context;
    if (directMacroContext != null) {
        throw new Error('prompt_assembly.macro_context_duplicated: macroContext must be read from FrozenRunInputSnapshot');
    }
    const promptInputs = requirePlainObject(
        frozenRunInputSnapshot.promptInputs,
        'prompt_assembly.prompt_inputs_required: promptInputs must be an object',
    );
    const macroContext = requirePlainObject(
        frozenRunInputSnapshot.macroContext,
        'prompt_assembly.macro_context_required: macroContext must be an object',
    );
    const settings = normalizeSettings(input);
    const agentContextPolicy = input.agentContextPolicy || input.contextPolicy
        ? normalizeAgentContextPolicy(input.agentContextPolicy ?? input.contextPolicy)
        : await loadAgentContextPolicy(profileId);
    const agentSystemPrompt = Object.prototype.hasOwnProperty.call(input, 'agentSystemPrompt')
        ? normalizeAgentSystemPrompt(input.agentSystemPrompt)
        : Object.prototype.hasOwnProperty.call(input, 'agent_system_prompt')
            ? normalizeAgentSystemPrompt(input.agent_system_prompt)
            : await loadResolvedAgentSystemPrompt(profileId);

    return {
        generationType,
        promptInputs,
        settings,
        modelId: normalizeOptionalString(input.modelId ?? input.model_id),
        jsonSchema: input.jsonSchema ?? input.json_schema ?? null,
        agentContextPolicy,
        agentSystemPrompt,
        worldInfoActivation: frozenRunInputSnapshot.worldInfoActivation,
        macroContext,
        frozenRunInputSnapshot,
    };
}

function normalizeRequiredFrozenRunInputSnapshot(input) {
    const snapshot = input.frozenRunInputSnapshot ?? input.frozen_run_input_snapshot;
    if (!snapshot || typeof snapshot !== 'object' || Array.isArray(snapshot)) {
        throw new Error('prompt_assembly.frozen_run_input_snapshot_required: frozenRunInputSnapshot must be an object');
    }
    return normalizeFrozenRunInputSnapshot(snapshot);
}

function normalizeSettings(input) {
    const settings = input.settings ?? input.presetSettings ?? input.preset_settings;
    if (settings == null) {
        throw new Error('prompt_assembly.settings_required: settings are required');
    }

    return requirePlainObject(settings, 'prompt_assembly.settings_invalid: settings must be an object');
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function normalizeOptionalString(value) {
    const text = String(value ?? '').trim();
    return text || undefined;
}

function requirePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    return value;
}

function hasChatCompletionSource(settings) {
    return typeof settings?.chat_completion_source === 'string'
        && settings.chat_completion_source.trim().length > 0;
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
        throw new Error('agent.prompt_snapshot_messages_required: prompt assembly did not produce chat-completion messages');
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
