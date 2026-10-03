// @ts-check

import {
    MODEL_TARGET_ID_PREFIX,
    MODEL_TARGET_KIND,
    MODEL_TARGET_SELECTION_KIND,
    listModelTargets,
    modelTargetApiSettings,
    modelTargetConnectionRef,
} from '../../../scripts/tauritavern/agent/model-target-llm-connection.js';

/**
 * @typedef {{ kind: typeof MODEL_TARGET_KIND, id: string }} ModelRef
 * @typedef {{
 *   ref: ModelRef;
 *   requestId: string;
 *   connectionRef: string | null;
 *   name: string;
 *   mode: 'cc' | 'tc';
 *   source: string;
 *   apiFormat: string | null;
 *   model: string;
 *   selected: boolean;
 * }} ModelSummary
 */

/** Connection Manager events that change the list or the selection. */
const MODEL_EVENTS = ['MODEL_TARGET_CREATED', 'MODEL_TARGET_UPDATED', 'MODEL_TARGET_DELETED', 'MODEL_TARGET_LOADED', 'CONNECTION_PROFILE_LOADED'];

/** @returns {any} */
function requireContext() {
    const context = /** @type {any} */ (window).SillyTavern?.getContext?.();
    if (!context) {
        throw new Error('models.context_unavailable: SillyTavern context is not ready');
    }
    return context;
}

/**
 * The saved models the user sees under "Model", as secret-free summaries.
 * `requestId` works with `ConnectionManagerRequestService`; `connectionRef` with Agent
 * Profiles (chat-completion models only). Source and API format are normalized the way
 * the model's Agent LLM connection is built.
 * @param {import('../../../scripts/tauritavern/agent/model-target-llm-connection.js').AgentModelTarget} target
 * @param {string | null} selectedId
 * @returns {ModelSummary}
 */
function summarize(target, selectedId) {
    // Connection Manager records `cc` or `tc` on every model it saves.
    const mode = /** @type {'cc' | 'tc'} */ (target.mode);
    const { chat_completion_source: source, custom_api_format, opencode_api_format } = modelTargetApiSettings(target);
    return {
        ref: { kind: MODEL_TARGET_KIND, id: String(target.id) },
        requestId: `${MODEL_TARGET_ID_PREFIX}${target.id}`,
        connectionRef: mode === 'cc' ? modelTargetConnectionRef(target) : null,
        name: String(target.name ?? ''),
        mode,
        source,
        apiFormat: custom_api_format || opencode_api_format || null,
        model: String(target.model ?? ''),
        selected: target.id === selectedId,
    };
}

/**
 * @param {any} context
 * @returns {ModelSummary[]}
 */
function listModels(context) {
    const selectedItem = context.extensionSettings?.connectionManager?.selectedItem;
    const selectedId = selectedItem?.kind === MODEL_TARGET_SELECTION_KIND ? selectedItem.id : null;
    return listModelTargets(context).map((target) => summarize(target, selectedId));
}

/**
 * @param {unknown} input A ModelRef, a request id (`modelTarget:<id>`) or a raw id
 * @returns {string}
 */
function requireTargetId(input) {
    const value = typeof input === 'object' && input !== null ? /** @type {any} */ (input).id : input;
    const id = String(value ?? '').trim();
    if (!id) {
        throw new Error('models.ref_required: a model ref or id is required');
    }
    return id.startsWith(MODEL_TARGET_ID_PREFIX) ? id.slice(MODEL_TARGET_ID_PREFIX.length) : id;
}

function createModelsApi() {
    return {
        /**
         * Async so the list can move to the backend without changing callers.
         * @returns {Promise<{ models: ModelSummary[] }>}
         */
        async list() {
            return { models: listModels(requireContext()) };
        },

        /**
         * @param {unknown} ref
         * @returns {Promise<ModelSummary | null>}
         */
        async get(ref) {
            const id = requireTargetId(ref);
            return listModels(requireContext()).find((model) => model.ref.id === id) ?? null;
        },

        /**
         * Calls `listener({ models })` whenever the list or the selection changes.
         * @param {(state: { models: ModelSummary[] }) => void} listener
         * @returns {() => void} Unsubscribe
         */
        subscribe(listener) {
            if (typeof listener !== 'function') {
                throw new Error('models.listener_required: subscribe expects a function');
            }
            const context = requireContext();
            const notify = () => listener({ models: listModels(context) });
            const eventTypes = MODEL_EVENTS.map((name) => {
                const eventType = context.eventTypes?.[name];
                if (!eventType) {
                    throw new Error(`models.event_unavailable: ${name}`);
                }
                context.eventSource.on(eventType, notify);
                return eventType;
            });
            return () => eventTypes.forEach((eventType) => context.eventSource.removeListener(eventType, notify));
        },
    };
}

/**
 * Installs `window.__TAURITAVERN__.api.models`.
 */
export function installModelsApi() {
    const hostAbi = /** @type {any} */ (window).__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }
    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }
    hostAbi.api.models = createModelsApi();
}
