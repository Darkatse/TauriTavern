export const MODEL_TARGET_KIND = 'tauritavern.modelTarget';

const LLM_CONNECTION_KIND = 'tauritavern.llmConnection';
const LLM_CONNECTION_SCHEMA_VERSION = 1;
const MODEL_TARGET_CONNECTION_PREFIX = 'model-target-';
const NO_PROXY_PRESET = 'None';

const CUSTOM_API_FORMAT_BY_API = Object.freeze({
    custom_openai_responses: 'openai_responses',
    custom_claude_messages: 'claude_messages',
    custom_gemini_interactions: 'gemini_interactions',
});

const SOURCE_ALIASES = Object.freeze({
    'open-router': 'openrouter',
    google: 'makersuite',
    gemini: 'makersuite',
    'vertex-ai': 'vertexai',
    'vertex ai': 'vertexai',
    'nano-gpt': 'nanogpt',
    'nano gpt': 'nanogpt',
    'silicon flow': 'siliconflow',
    'workers-ai': 'workers_ai',
    'workers ai': 'workers_ai',
    'cloudflare workers ai': 'workers_ai',
    'z.ai': 'zai',
    glm: 'zai',
    'mini-max': 'minimax',
    'mini max': 'minimax',
});

const SOURCE_SPECIFIC_API_URL_KEYS = Object.freeze({
    zai: 'zai_endpoint',
    siliconflow: 'siliconflow_endpoint',
    minimax: 'minimax_endpoint',
    vertexai: 'vertexai_region',
});

export function listSavedModelTargets(context = requireSillyTavernContext()) {
    const targets = context.extensionSettings?.connectionManager?.modelTargets;
    if (!Array.isArray(targets)) {
        return [];
    }

    return targets
        .filter((target) => target?.kind === MODEL_TARGET_KIND && target.mode === 'cc')
        .map((target) => structuredClone(target))
        .sort((a, b) => String(a.name || '').localeCompare(String(b.name || '')));
}

export function modelTargetConnectionRef(target) {
    const rawId = String(target?.id || '').trim();
    if (!rawId) {
        throw new Error('model target id is required');
    }

    const normalized = rawId
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, '-')
        .replace(/^-+|-+$/g, '');
    if (!normalized) {
        throw new Error(`invalid model target id: ${rawId}`);
    }

    const connectionRef = `${MODEL_TARGET_CONNECTION_PREFIX}${normalized}`;
    if (connectionRef.length > 128) {
        throw new Error(`model target id is too long for an Agent LLM connection: ${rawId}`);
    }

    return connectionRef;
}

export function modelTargetIdFromConnectionRef(connectionRef) {
    const value = String(connectionRef || '').trim();
    if (!value.startsWith(MODEL_TARGET_CONNECTION_PREFIX)) {
        return '';
    }
    return value.slice(MODEL_TARGET_CONNECTION_PREFIX.length);
}

export function modelBindingFromTarget(target) {
    assertModelTargetConvertible(target);
    return {
        mode: 'connectionRef',
        connectionRef: modelTargetConnectionRef(target),
        modelId: String(target.model).trim(),
    };
}

export function buildLlmConnectionFromModelTarget(target) {
    assertModelTargetConvertible(target);

    const source = normalizeChatCompletionSource(target.api);
    const endpoint = {
        sourceSpecific: {},
    };
    const apiUrl = String(target['api-url'] || '').trim();
    if (apiUrl && source === 'custom') {
        endpoint.baseUrl = apiUrl;
    } else if (apiUrl && SOURCE_SPECIFIC_API_URL_KEYS[source]) {
        endpoint.sourceSpecific[SOURCE_SPECIFIC_API_URL_KEYS[source]] = apiUrl;
    }

    if (source === 'vertexai' && target.secretRef.key === 'api_key_vertexai_service_account') {
        endpoint.sourceSpecific.vertexai_auth_mode = 'full';
    }

    const customApiFormat = normalizeCustomApiFormat(target);

    return {
        schemaVersion: LLM_CONNECTION_SCHEMA_VERSION,
        kind: LLM_CONNECTION_KIND,
        id: modelTargetConnectionRef(target),
        displayName: String(target.name || target.model).trim(),
        description: `Connection Manager model target: ${String(target.name || target.id).trim()}`,
        provider: {
            chatCompletionSource: source,
            ...(customApiFormat ? { customApiFormat } : {}),
        },
        endpoint,
        auth: {
            secretRef: {
                key: String(target.secretRef.key).trim(),
                id: String(target.secretRef.id).trim(),
                ...(String(target.secretRef.labelSnapshot || '').trim()
                    ? { labelSnapshot: String(target.secretRef.labelSnapshot).trim() }
                    : {}),
            },
        },
        routing: {},
        adapterHints: {},
        capabilities: {},
    };
}

export function findModelTargetForBinding(modelTargets, model) {
    if (!model || model.mode !== 'connectionRef') {
        return null;
    }

    const connectionRef = String(model.connectionRef || '').trim();
    if (!modelTargetIdFromConnectionRef(connectionRef)) {
        return null;
    }

    const target = findModelTargetForConnectionRef(modelTargets, connectionRef);
    if (!target || target.model !== model.modelId) {
        return null;
    }
    return target;
}

export function findModelTargetForConnectionRef(modelTargets, connectionRef) {
    const normalizedConnectionRef = String(connectionRef || '').trim();
    if (!modelTargetIdFromConnectionRef(normalizedConnectionRef)) {
        return null;
    }

    return modelTargets.find((target) => modelTargetConnectionRef(target) === normalizedConnectionRef) || null;
}

export async function saveModelTargetAsLlmConnection(target, llmConnectionsApi = requireLlmConnectionsApi()) {
    const connection = buildLlmConnectionFromModelTarget(target);
    await llmConnectionsApi.save({ connection });
    return connection;
}

export async function syncModelTargetLlmConnection(target, deps = {}) {
    return saveModelTargetAsLlmConnection(
        target,
        deps.llmConnectionsApi || requireLlmConnectionsApi(),
    );
}

export async function ensureModelTargetLlmConnectionForProfile(profile, deps = {}) {
    const model = profile?.model;
    if (!isModelTargetBinding(model)) {
        return null;
    }

    const modelTargets = Array.isArray(deps.modelTargets)
        ? deps.modelTargets
        : listSavedModelTargets(deps.context || requireSillyTavernContext());
    const target = findModelTargetForConnectionRef(modelTargets, model.connectionRef);
    if (!target) {
        throw new Error(`agent.model_target_binding_missing: Model Target binding '${model.connectionRef}' for model '${model.modelId}' was not found`);
    }

    return saveModelTargetAsLlmConnection(
        target,
        deps.llmConnectionsApi || requireLlmConnectionsApi(),
    );
}

function isModelTargetBinding(model) {
    return Boolean(
        model?.mode === 'connectionRef'
        && modelTargetIdFromConnectionRef(model.connectionRef),
    );
}

function assertModelTargetConvertible(target) {
    if (!target || typeof target !== 'object' || Array.isArray(target)) {
        throw new Error('model target must be an object');
    }
    if (target.kind !== MODEL_TARGET_KIND) {
        throw new Error(`invalid model target kind: ${target.kind}`);
    }
    if (target.mode !== 'cc') {
        throw new Error(`model target "${target.name || target.id}" is not a chat-completion target`);
    }
    if (!String(target.api || '').trim()) {
        throw new Error(`model target "${target.name || target.id}" is missing API`);
    }
    if (!String(target.model || '').trim()) {
        throw new Error(`model target "${target.name || target.id}" is missing model`);
    }
    if (!target.secretRef?.key || !target.secretRef?.id) {
        throw new Error(`model target "${target.name || target.id}" is missing secret reference`);
    }
    const proxy = String(target.proxy || '').trim();
    if (proxy && proxy !== NO_PROXY_PRESET) {
        throw new Error(`model target "${target.name || target.id}" uses proxy preset "${proxy}", which cannot be converted to an Agent LLM connection yet`);
    }
}

function normalizeChatCompletionSource(value) {
    const source = String(value || '').trim().toLowerCase();
    if (!source) {
        return '';
    }
    if (CUSTOM_API_FORMAT_BY_API[source]) {
        return 'custom';
    }
    return SOURCE_ALIASES[source] || source;
}

function normalizeCustomApiFormat(target) {
    const api = String(target.api || '').trim().toLowerCase();
    const explicit = String(target['custom-api-format'] || '').trim();
    if (explicit) {
        return explicit;
    }
    if (CUSTOM_API_FORMAT_BY_API[api]) {
        return CUSTOM_API_FORMAT_BY_API[api];
    }
    return normalizeChatCompletionSource(target.api) === 'custom' ? 'openai_compat' : '';
}

function requireSillyTavernContext() {
    const context = globalThis.window?.SillyTavern?.getContext?.();
    if (!context || typeof context !== 'object') {
        throw new Error('agent.model_target_context_unavailable: SillyTavern context is required to resolve Model Target LLM connection');
    }
    return context;
}

function requireLlmConnectionsApi() {
    const llmConnectionsApi = globalThis.window?.__TAURITAVERN__?.api?.llmConnections;
    if (typeof llmConnectionsApi?.save !== 'function') {
        throw new Error('agent.llm_connection_api_unavailable: LLM Connection API is unavailable');
    }
    return llmConnectionsApi;
}
