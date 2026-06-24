export const FROZEN_RUN_INPUT_SNAPSHOT_KIND = 'tauritavern.agentFrozenRunInputSnapshot';
export const FROZEN_RUN_INPUT_SNAPSHOT_SCHEMA_VERSION = 1;
export const CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND = 'tauritavern.currentModelConnectionSnapshot';
export const CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION = 1;

export function buildFrozenRunInputSnapshot({
    generationType,
    promptInputs,
    worldInfoActivation,
    macroContext,
    currentModelConnection,
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
        ...(currentModelConnection ? { currentModelConnection: normalizeCurrentModelConnectionSnapshot(currentModelConnection) } : {}),
    };
}

export async function buildCurrentModelConnectionSnapshot({
    settings,
    model,
    secretKey,
    secretState,
    currentModelConnectionApi,
} = {}) {
    const settingsSnapshot = clonePlainObject(
        settings,
        'agent.current_model_connection_settings_invalid: settings must be a structured-cloneable object',
    );
    const source = normalizeNonEmptyString(
        settingsSnapshot.chat_completion_source,
        'agent.current_model_connection_source_required: chat_completion_source cannot be empty',
    );
    const resolvedModel = normalizeNonEmptyString(
        model,
        'agent.current_model_connection_model_required: model cannot be empty',
    );
    const activeSecretId = getActiveSecretId(secretState, secretKey);
    const api = currentModelConnectionApi || requireCurrentModelConnectionApi();
    const snapshot = await api.buildCurrentModelConnectionSnapshot({
        settings: {
            ...settingsSnapshot,
            chat_completion_source: source,
        },
        model: resolvedModel,
        ...(activeSecretId ? { secretId: activeSecretId } : {}),
    });

    return normalizeCurrentModelConnectionSnapshot(snapshot);
}

export async function buildSettingsWithCurrentModelConnectionSnapshot(settings, currentModelConnection, currentModelConnectionApi) {
    const baseSettings = clonePlainObject(settings, 'agent.current_model_connection_base_settings_invalid: settings must be a structured-cloneable object');
    const snapshot = normalizeCurrentModelConnectionSnapshot(currentModelConnection);
    const api = currentModelConnectionApi || requireCurrentModelConnectionApi();
    return clonePlainObject(
        await api.applyCurrentModelConnectionSnapshot({
            settings: baseSettings,
            currentModelConnection: snapshot,
        }),
        'agent.current_model_connection_applied_settings_invalid: backend settings must be an object',
    );
}

export function normalizeCurrentModelConnectionSnapshot(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('agent.current_model_connection_snapshot_required: currentModelConnection must be an object');
    }

    const schemaVersion = Number(value.schemaVersion ?? value.schema_version);
    if (schemaVersion !== CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION) {
        throw new Error(`agent.current_model_connection_snapshot_schema_unsupported: schemaVersion ${schemaVersion} is unsupported`);
    }

    const kind = String(value.kind || '').trim();
    if (kind !== CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND) {
        throw new Error(`agent.current_model_connection_snapshot_kind_invalid: kind must be ${CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND}`);
    }

    const rawSettings = clonePlainObject(
        value.settings,
        'agent.current_model_connection_settings_invalid: settings must be a structured-cloneable object',
    );
    const settings = rawSettings;
    settings.chat_completion_source = normalizeNonEmptyString(
        settings.chat_completion_source,
        'agent.current_model_connection_source_required: chat_completion_source cannot be empty',
    );
    settings.model = normalizeNonEmptyString(
        settings.model,
        'agent.current_model_connection_model_required: model cannot be empty',
    );
    if (Object.prototype.hasOwnProperty.call(settings, 'secret_id')) {
        const secretId = String(settings.secret_id ?? '').trim();
        if (secretId) {
            settings.secret_id = secretId;
        } else {
            delete settings.secret_id;
        }
    }

    return {
        schemaVersion: CURRENT_MODEL_CONNECTION_SNAPSHOT_SCHEMA_VERSION,
        kind: CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
        settings,
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
        ...((value.currentModelConnection ?? value.current_model_connection)
            ? { currentModelConnection: normalizeCurrentModelConnectionSnapshot(value.currentModelConnection ?? value.current_model_connection) }
            : {}),
    };
}

function normalizeGenerationType(value) {
    const generationType = String(value || 'normal').trim();
    if (!generationType) {
        throw new Error('agent.frozen_run_input_generation_type_empty: generationType cannot be empty');
    }
    return generationType;
}

function normalizeNonEmptyString(value, message) {
    const text = String(value ?? '').trim();
    if (!text) {
        throw new Error(message);
    }
    return text;
}

function clonePlainObject(value, message) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(message);
    }
    const clone = cloneStructuredValue(value, message);
    if (!clone || typeof clone !== 'object' || Array.isArray(clone)) {
        throw new Error(message);
    }
    return clone;
}

function cloneStructuredValue(value, message) {
    try {
        return structuredClone(value);
    } catch {
        throw new Error(message);
    }
}

function requireCurrentModelConnectionApi() {
    const api = globalThis.window?.__TAURITAVERN__?.api?.agent?.promptAssembly;
    if (
        typeof api?.buildCurrentModelConnectionSnapshot !== 'function'
        || typeof api?.applyCurrentModelConnectionSnapshot !== 'function'
    ) {
        throw new Error('agent.current_model_connection_api_unavailable: TauriTavern Agent prompt assembly API is unavailable');
    }
    return api;
}

function getActiveSecretId(secretState, secretKey) {
    const key = String(secretKey ?? '').trim();
    if (!key || !secretState || typeof secretState !== 'object') {
        return null;
    }

    const secrets = secretState[key];
    if (!Array.isArray(secrets)) {
        return null;
    }

    const activeSecret = secrets.find(secret => secret?.active);
    if (!activeSecret) {
        return null;
    }

    const id = String(activeSecret.id ?? '').trim();
    if (!id) {
        throw new Error(`agent.current_model_connection_secret_id_invalid: active secret for ${key} has no id`);
    }
    return id;
}
