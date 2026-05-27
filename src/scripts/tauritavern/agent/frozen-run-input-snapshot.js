export const FROZEN_RUN_INPUT_SNAPSHOT_KIND = 'tauritavern.agentFrozenRunInputSnapshot';
export const FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION = 1;

export function buildFrozenRunInputSnapshot({
    generationType,
    promptInputs,
    worldInfoActivation,
    macroContext,
} = {}) {
    const normalizedGenerationType = normalizeGenerationType(generationType ?? promptInputs?.type);
    const frozenPromptInputs = clonePlainObject(promptInputs, 'agent.frozen_run_input_prompt_inputs_invalid: promptInputs must be a structured-cloneable object');
    const frozenWorldInfoActivation = clonePlainObject(worldInfoActivation, 'agent.frozen_run_input_world_info_activation_invalid: worldInfoActivation must be a structured-cloneable object');
    const frozenMacroContext = clonePlainObject(macroContext, 'agent.frozen_run_input_macro_context_invalid: macroContext must be a structured-cloneable object');

    return {
        schemaVersion: FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION,
        kind: FROZEN_RUN_INPUT_SNAPSHOT_KIND,
        generationType: normalizedGenerationType,
        promptInputs: frozenPromptInputs,
        worldInfoActivation: frozenWorldInfoActivation,
        macroContext: frozenMacroContext,
    };
}

export async function snapshotExtensionPromptsForFrozenRun(extensionPrompts) {
    if (!extensionPrompts || typeof extensionPrompts !== 'object' || Array.isArray(extensionPrompts)) {
        throw new Error('agent.extension_prompts_invalid: extensionPrompts must be an object');
    }

    const snapshot = {};
    for (const [key, prompt] of Object.entries(extensionPrompts)) {
        if (!prompt || typeof prompt !== 'object' || Array.isArray(prompt)) {
            throw new Error(`agent.extension_prompt_invalid: extension prompt ${key} must be an object`);
        }

        if (typeof prompt.filter === 'function' && !await prompt.filter()) {
            continue;
        }

        const value = prompt.value == null ? '' : String(prompt.value);
        if (!value) {
            continue;
        }

        snapshot[key] = {
            value,
            position: Number(prompt.position),
            depth: Number(prompt.depth),
            scan: Boolean(prompt.scan),
            role: Number(prompt.role),
        };
    }

    return clonePlainObject(snapshot, 'agent.extension_prompts_snapshot_invalid: extensionPrompts snapshot must be structured-cloneable');
}

export function normalizeFrozenRunInputSnapshot(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('agent.frozen_run_input_snapshot_required: FrozenRunInputSnapshot must be an object');
    }

    const schemaVersion = Number(value.schemaVersion ?? value.schema_version);
    if (schemaVersion !== FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION) {
        throw new Error(`agent.frozen_run_input_snapshot_schema_unsupported: schemaVersion ${schemaVersion} is unsupported`);
    }

    const kind = String(value.kind || '').trim();
    if (kind !== FROZEN_RUN_INPUT_SNAPSHOT_KIND) {
        throw new Error(`agent.frozen_run_input_snapshot_kind_invalid: kind must be ${FROZEN_RUN_INPUT_SNAPSHOT_KIND}`);
    }

    const generationType = normalizeGenerationType(value.generationType ?? value.generation_type);
    const promptInputs = clonePlainObject(
        value.promptInputs ?? value.prompt_inputs,
        'agent.frozen_run_input_prompt_inputs_invalid: promptInputs must be a structured-cloneable object',
    );
    const worldInfoActivation = clonePlainObject(
        value.worldInfoActivation ?? value.world_info_activation,
        'agent.frozen_run_input_world_info_activation_invalid: worldInfoActivation must be a structured-cloneable object',
    );
    const macroContext = clonePlainObject(
        value.macroContext ?? value.macro_context,
        'agent.frozen_run_input_macro_context_invalid: macroContext must be a structured-cloneable object',
    );

    return {
        schemaVersion: FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION,
        kind: FROZEN_RUN_INPUT_SNAPSHOT_KIND,
        generationType,
        promptInputs,
        worldInfoActivation,
        macroContext,
    };
}

function normalizeGenerationType(value) {
    const generationType = String(value || 'normal').trim();
    if (!generationType) {
        throw new Error('agent.frozen_run_input_generation_type_empty: generationType cannot be empty');
    }
    return generationType;
}

function clonePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    const clone = structuredClone(value);
    if (!clone || typeof clone !== 'object' || Array.isArray(clone)) {
        throw new Error(message);
    }
    return clone;
}
