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
import { createAgentPromptSnapshot } from '../../../scripts/tauritavern/agent/agent-model-messages.js';
import { setParamOmitted } from '../../../scripts/tauri/generation-params/omission.js';

const PROMPT_ASSEMBLY_SOURCE = 'frontend-prompt-assembly-broker';

/**
 * Builds an Agent PromptSnapshot through the real SillyTavern chat-completion
 * PromptManager pipeline, using frozen prompt inputs and preset settings.
 *
 * @param {Record<string, unknown>} input
 */
export async function buildPromptAssemblySnapshot(input = {}) {
    const { payload, snapshotMetadata, ...assembled } = await buildPromptAssemblyPayload(input);
    return {
        ...assembled,
        promptSnapshot: createAgentPromptSnapshot(payload, snapshotMetadata),
    };
}

/**
 * Raw assembly boundary for Chat's compatibility event, before canonical conversion.
 * @param {Record<string, unknown>} input
 */
export async function buildPromptAssemblyPayload(input = {}) {
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
    if (request.reasoningEffort) {
        settings.reasoning_effort = request.reasoningEffort;
        setParamOmitted(settings, 'reasoning_effort', false);
    }

    const result = await openai.assembleOpenAIChatCompletionPrompt({
        settings,
        model,
        generationType: request.generationType,
        promptInputs: request.promptInputs,
        macroContext: request.macroContext,
        jsonSchema: request.jsonSchema,
        agentMode: true,
        agentContextPolicy: request.agentContextPolicy,
        agentSystemPrompt: request.agentSystemPrompt,
        agentTaskPrompt: request.agentTaskPrompt,
        contextKind: request.frozenRunInputSnapshot.contextKind ?? 'chat',
    });

    if (request.frozenRunInputSnapshot.contextKind === 'session') {
        // JSONL owns the full history; the Run retains only the budgeted prompt messages.
        delete request.frozenRunInputSnapshot.promptInputs.agentMessages;
    }

    const payload = result.chatCompletionPayload;
    return {
        payload,
        snapshotMetadata: {
            contextPolicy: request.agentContextPolicy,
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

/** @param {Record<string, unknown>} input */
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
    const agentTaskPrompt = Object.prototype.hasOwnProperty.call(input, 'agentTaskPrompt')
        ? normalizeOptionalPrompt(input.agentTaskPrompt)
        : Object.prototype.hasOwnProperty.call(input, 'agent_task_prompt')
            ? normalizeOptionalPrompt(input.agent_task_prompt)
            : null;
    const requiredAgentPromptComponents = normalizeRequiredAgentPromptComponents(
        input.requiredAgentPromptComponents ?? input.required_agent_prompt_components,
    );
    if (requiredAgentPromptComponents.includes('agentTask') && !agentTaskPrompt) {
        throw new Error('agent.task_prompt_required: prompt assembly request requires agentTaskPrompt');
    }

    return {
        generationType,
        promptInputs,
        settings,
        modelId: normalizeOptionalString(input.modelId ?? input.model_id),
        reasoningEffort: normalizeOptionalString(input.reasoningEffort),
        jsonSchema: input.jsonSchema ?? input.json_schema ?? null,
        agentContextPolicy,
        agentSystemPrompt,
        agentTaskPrompt,
        requiredAgentPromptComponents,
        worldInfoActivation: frozenRunInputSnapshot.worldInfoActivation,
        macroContext,
        frozenRunInputSnapshot,
    };
}

/** @param {Record<string, unknown>} input */
function normalizeRequiredFrozenRunInputSnapshot(input) {
    const snapshot = input.frozenRunInputSnapshot ?? input.frozen_run_input_snapshot;
    if (!snapshot || typeof snapshot !== 'object' || Array.isArray(snapshot)) {
        throw new Error('prompt_assembly.frozen_run_input_snapshot_required: frozenRunInputSnapshot must be an object');
    }
    return normalizeFrozenRunInputSnapshot(snapshot);
}

/** @param {Record<string, unknown>} input */
function normalizeSettings(input) {
    const settings = input.settings ?? input.presetSettings ?? input.preset_settings;
    if (settings == null) {
        throw new Error('prompt_assembly.settings_required: settings are required');
    }

    return requirePlainObject(settings, 'prompt_assembly.settings_invalid: settings must be an object');
}

/** @param {unknown} value */
function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

/** @param {unknown} value */
function normalizeOptionalString(value) {
    const text = String(value ?? '').trim();
    return text || undefined;
}

/** @param {unknown} value */
function normalizeOptionalPrompt(value) {
    const text = String(value ?? '');
    return text.trim() ? text : null;
}

/** @param {unknown} value */
function normalizeRequiredAgentPromptComponents(value) {
    if (value == null) {
        return [];
    }
    if (!Array.isArray(value)) {
        throw new Error('prompt_assembly.required_components_invalid: requiredAgentPromptComponents must be an array');
    }
    return value
        .map(component => String(component || '').trim())
        .filter(Boolean);
}

/**
 * @param {unknown} value
 * @param {string} message
 * @returns {Record<string, unknown>}
 */
function requirePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    return /** @type {Record<string, unknown>} */ (value);
}

/** @param {Record<string, unknown>} settings */
function hasChatCompletionSource(settings) {
    return typeof settings?.chat_completion_source === 'string'
        && settings.chat_completion_source.trim().length > 0;
}
