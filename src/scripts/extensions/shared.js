import { CONNECT_API_MAP, createModelIcon, getRequestHeaders } from '../../script.js';
import { extension_settings, openThirdPartyExtensionMenu } from '../extensions.js';
import { t } from '../i18n.js';
import { applyAdditionalParametersToRequest, oai_settings, POLLINATIONS_ENDPOINT, proxies, ZAI_ENDPOINT } from '../openai.js';
import { SECRET_KEYS, secret_state } from '../secrets.js';
import { textgen_types, textgenerationwebui_settings } from '../textgen-settings.js';
import { getTokenCountAsync } from '../tokenizers.js';
import { createThumbnail, isValidUrl } from '../utils.js';
import { MODEL_TARGET_ID_PREFIX, MODEL_TARGET_SELECTION_KIND } from '../tauritavern/agent/model-target-llm-connection.js';

export const NATIVE_CAPTION_UNAVAILABLE_MESSAGE = 'Image captioning is not implemented in the TauriTavern native backend.';

async function getCaptionErrorMessage(response) {
    const fallback = `Failed to caption image via Multimodal API. HTTP ${response.status}`;
    const text = await response.text();
    if (!text) {
        return fallback;
    }

    try {
        const data = JSON.parse(text);
        if (typeof data.message === 'string' && data.message.trim()) {
            return data.message;
        }
        if (typeof data.error === 'string' && data.error.trim()) {
            return data.error;
        }
        if (typeof data.error?.message === 'string' && data.error.message.trim()) {
            return data.error.message;
        }
        return `${fallback}: ${text}`;
    } catch {
        return text;
    }
}

/**
 * Generates a caption for an image using a multimodal model.
 * @param {string} base64Img Base64 encoded image
 * @param {string} prompt Prompt to use for captioning
 * @param {AbortSignal} [signal] Signal used to cancel captioning
 * @returns {Promise<string>} Generated caption
 */
export async function getMultimodalCaption(base64Img, prompt, signal) {
    const useReverseProxy =
        (['openai', 'anthropic', 'google', 'mistral', 'vertexai', 'xai', 'zai', 'moonshot'].includes(extension_settings.caption.multimodal_api))
        && extension_settings.caption.allow_reverse_proxy
        && oai_settings.reverse_proxy
        && isValidUrl(oai_settings.reverse_proxy);

    if (globalThis.__TAURI_RUNNING__ !== true) {
        throwIfInvalidModel(useReverseProxy);
    }

    // OpenRouter has a payload limit of ~2MB. Google is 4MB, but we love democracy.
    // Ooba requires all images to be JPEGs. Koboldcpp just asked nicely.
    const isOllama = extension_settings.caption.multimodal_api === 'ollama';
    const isLlamaCpp = extension_settings.caption.multimodal_api === 'llamacpp';
    const isCustom = extension_settings.caption.multimodal_api === 'custom';
    const isOoba = extension_settings.caption.multimodal_api === 'ooba';
    const isKoboldCpp = extension_settings.caption.multimodal_api === 'koboldcpp';
    const isVllm = extension_settings.caption.multimodal_api === 'vllm';
    const base64Bytes = base64Img.length * 0.75;
    const compressionLimit = 2 * 1024 * 1024;
    const safeMimeTypes = ['image/jpeg', 'image/png', 'image/webp'];
    const mimeType = base64Img?.split(';')?.[0]?.split(':')?.[1] || 'image/jpeg';
    const isImage = mimeType.startsWith('image/');
    const thumbnailNeeded = ['google', 'openrouter', 'mistral', 'groq', 'vertexai'].includes(extension_settings.caption.multimodal_api);
    if ((isImage && thumbnailNeeded && base64Bytes > compressionLimit) || isOoba || isKoboldCpp) {
        const maxSide = 2048;
        base64Img = await createThumbnail(base64Img, maxSide, maxSide);
    } else if (isImage && !safeMimeTypes.includes(mimeType)) {
        base64Img = await createThumbnail(base64Img, null, null);
    }
    if (isOllama && base64Img.startsWith('data:image/')) {
        base64Img = base64Img.split(',')[1];
    }

    const proxyUrl = useReverseProxy ? oai_settings.reverse_proxy : '';
    const proxyPassword = useReverseProxy ? oai_settings.proxy_password : '';

    const requestBody = {
        image: base64Img,
        prompt: prompt,
        reverse_proxy: proxyUrl,
        proxy_password: proxyPassword,
        api: extension_settings.caption.multimodal_api || 'openai',
        model: extension_settings.caption.multimodal_model || 'gpt-4-turbo',
    };

    // Add Vertex AI specific parameters if using Vertex AI
    if (extension_settings.caption.multimodal_api === 'vertexai') {
        requestBody.vertexai_auth_mode = oai_settings.vertexai_auth_mode;
        requestBody.vertexai_region = oai_settings.vertexai_region;
        requestBody.vertexai_express_project_id = oai_settings.vertexai_express_project_id;
    }

    if (isOllama) {
        if (extension_settings.caption.multimodal_model === 'ollama_current') {
            requestBody.model = textgenerationwebui_settings.ollama_model;
        }

        if (extension_settings.caption.multimodal_model === 'ollama_custom') {
            requestBody.model = extension_settings.caption.ollama_custom_model;
        }

        requestBody.server_url = extension_settings.caption.alt_endpoint_enabled
            ? extension_settings.caption.alt_endpoint_url
            : textgenerationwebui_settings.server_urls[textgen_types.OLLAMA];
    }

    if (isVllm) {
        if (extension_settings.caption.multimodal_model === 'vllm_current') {
            requestBody.model = textgenerationwebui_settings.vllm_model;
        }

        requestBody.server_url = extension_settings.caption.alt_endpoint_enabled
            ? extension_settings.caption.alt_endpoint_url
            : textgenerationwebui_settings.server_urls[textgen_types.VLLM];
    }

    if (isLlamaCpp) {
        requestBody.server_url = extension_settings.caption.alt_endpoint_enabled
            ? extension_settings.caption.alt_endpoint_url
            : textgenerationwebui_settings.server_urls[textgen_types.LLAMACPP];
    }

    if (isOoba) {
        requestBody.server_url = extension_settings.caption.alt_endpoint_enabled
            ? extension_settings.caption.alt_endpoint_url
            : textgenerationwebui_settings.server_urls[textgen_types.OOBA];
    }

    if (isKoboldCpp) {
        requestBody.server_url = extension_settings.caption.alt_endpoint_enabled
            ? extension_settings.caption.alt_endpoint_url
            : textgenerationwebui_settings.server_urls[textgen_types.KOBOLDCPP];
    }

    if (isCustom) {
        if (extension_settings.caption.multimodal_model === 'custom_current') {
            requestBody.model = oai_settings.custom_model || '';
        }

        if (extension_settings.caption.multimodal_model === 'custom_custom') {
            requestBody.model = extension_settings.caption.custom_model || '';
        }

        requestBody.server_url = oai_settings.custom_url;
        applyAdditionalParametersToRequest(requestBody, oai_settings, { sourceKey: 'custom' });
    }

    if (extension_settings.caption.multimodal_api === 'zai') {
        requestBody.zai_endpoint = oai_settings.zai_endpoint || ZAI_ENDPOINT.COMMON;
    }

    if (extension_settings.caption.multimodal_api === 'pollinations') {
        requestBody.pollinations_endpoint = oai_settings.pollinations_endpoint || POLLINATIONS_ENDPOINT.AUTHENTICATED;
    }

    if (extension_settings.caption.multimodal_api === 'workers_ai') {
        requestBody.workers_ai_account_id = oai_settings.workers_ai_account_id;
    }

    if (extension_settings.caption.multimodal_api === 'moonshot') {
        requestBody.moonshot_endpoint = oai_settings.moonshot_endpoint;
    }

    function getEndpointUrl() {
        switch (extension_settings.caption.multimodal_api) {
            case 'google':
            case 'vertexai':
                return '/api/google/caption-image';
            case 'anthropic':
                return '/api/anthropic/caption-image';
            case 'ollama':
                return '/api/backends/text-completions/ollama/caption-image';
            default:
                return '/api/openai/caption-image';
        }
    }

    const apiResult = await fetch(getEndpointUrl(), {
        method: 'POST',
        headers: getRequestHeaders(),
        body: JSON.stringify(requestBody),
        signal,
    });

    if (!apiResult.ok) {
        throw new Error(await getCaptionErrorMessage(apiResult));
    }

    const { caption } = await apiResult.json();
    if (typeof caption !== 'string' || !caption.trim()) {
        throw new Error('Image captioning response is missing caption text.');
    }
    return caption.trim();
}

function throwIfInvalidModel(useReverseProxy) {
    const altEndpointEnabled = extension_settings.caption.alt_endpoint_enabled;
    const altEndpointUrl = extension_settings.caption.alt_endpoint_url;
    const multimodalModel = extension_settings.caption.multimodal_model;
    const multimodalApi = extension_settings.caption.multimodal_api;

    if (altEndpointEnabled && ['llamacpp', 'ooba', 'koboldcpp', 'vllm', 'ollama'].includes(multimodalApi) && !altEndpointUrl) {
        throw new Error('Secondary endpoint URL is not set.');
    }

    if (multimodalApi === 'openai' && !secret_state[SECRET_KEYS.OPENAI] && !useReverseProxy) {
        throw new Error('OpenAI API key is not set.');
    }

    if (multimodalApi === 'openrouter' && !secret_state[SECRET_KEYS.OPENROUTER]) {
        throw new Error('OpenRouter API key is not set.');
    }

    if (multimodalApi === 'anthropic' && !secret_state[SECRET_KEYS.CLAUDE] && !useReverseProxy) {
        throw new Error('Anthropic (Claude) API key is not set.');
    }

    if (multimodalApi === 'groq' && !secret_state[SECRET_KEYS.GROQ]) {
        throw new Error('Groq API key is not set.');
    }

    if (multimodalApi === 'google' && !secret_state[SECRET_KEYS.MAKERSUITE] && !useReverseProxy) {
        throw new Error('Google AI Studio API key is not set.');
    }

    if (multimodalApi === 'vertexai' && !useReverseProxy) {
        // Check based on authentication mode
        const authMode = oai_settings.vertexai_auth_mode || 'express';

        if (authMode === 'express') {
            // Express mode requires API key
            if (!secret_state[SECRET_KEYS.VERTEXAI]) {
                throw new Error('Google Vertex AI API key is not set for Express mode.');
            }
        } else if (authMode === 'full') {
            // Full mode requires Service Account JSON and region settings
            if (!secret_state[SECRET_KEYS.VERTEXAI_SERVICE_ACCOUNT]) {
                throw new Error('Service Account JSON is required for Vertex AI Full mode. Please validate and save your Service Account JSON.');
            }
            if (!oai_settings.vertexai_region) {
                throw new Error('Region is required for Vertex AI Full mode.');
            }
        }
    }

    if (multimodalApi === 'mistral' && !secret_state[SECRET_KEYS.MISTRALAI] && !useReverseProxy) {
        throw new Error('Mistral AI API key is not set.');
    }

    if (multimodalApi === 'cohere' && !secret_state[SECRET_KEYS.COHERE]) {
        throw new Error('Cohere API key is not set.');
    }

    if (multimodalApi === 'xai' && !secret_state[SECRET_KEYS.XAI] && !useReverseProxy) {
        throw new Error('xAI API key is not set.');
    }

    if (multimodalApi === 'ollama' && !textgenerationwebui_settings.server_urls[textgen_types.OLLAMA] && !altEndpointEnabled) {
        throw new Error('Ollama server URL is not set.');
    }

    if (multimodalApi === 'ollama' && multimodalModel === 'ollama_current' && !textgenerationwebui_settings.ollama_model) {
        throw new Error('Ollama model is not set.');
    }

    if (multimodalApi === 'ollama' && multimodalModel === 'ollama_custom' && !extension_settings.caption.ollama_custom_model) {
        throw new Error('Ollama custom model tag is not set.');
    }

    if (multimodalApi === 'llamacpp' && !textgenerationwebui_settings.server_urls[textgen_types.LLAMACPP] && !altEndpointEnabled) {
        throw new Error('LlamaCPP server URL is not set.');
    }

    if (multimodalApi === 'ooba' && !textgenerationwebui_settings.server_urls[textgen_types.OOBA] && !altEndpointEnabled) {
        throw new Error('Text Generation WebUI server URL is not set.');
    }

    if (multimodalApi === 'koboldcpp' && !textgenerationwebui_settings.server_urls[textgen_types.KOBOLDCPP] && !altEndpointEnabled) {
        throw new Error('KoboldCpp server URL is not set.');
    }

    if (multimodalApi === 'vllm' && !textgenerationwebui_settings.server_urls[textgen_types.VLLM] && !altEndpointEnabled) {
        throw new Error('vLLM server URL is not set.');
    }

    if (multimodalApi === 'vllm' && multimodalModel === 'vllm_current' && !textgenerationwebui_settings.vllm_model) {
        throw new Error('vLLM model is not set.');
    }

    if (multimodalApi === 'custom' && !oai_settings.custom_url) {
        throw new Error('Custom API URL is not set.');
    }

    if (multimodalApi === 'custom' && multimodalModel === 'custom_custom' && !extension_settings.caption.custom_model) {
        throw new Error('Custom OpenAI-compatible Model ID is not set.');
    }

    if (multimodalApi === 'aimlapi' && !secret_state[SECRET_KEYS.AIMLAPI]) {
        throw new Error('AI/ML API key is not set.');
    }

    if (multimodalApi === 'moonshot' && !secret_state[SECRET_KEYS.MOONSHOT]) {
        throw new Error('Moonshot AI API key is not set.');
    }

    if (multimodalApi === 'nanogpt' && !secret_state[SECRET_KEYS.NANOGPT]) {
        throw new Error('NanoGPT API key is not set.');
    }

    if (multimodalApi === 'electronhub' && !secret_state[SECRET_KEYS.ELECTRONHUB]) {
        throw new Error('Electron Hub API key is not set.');
    }

    if (multimodalApi === 'chutes' && !secret_state[SECRET_KEYS.CHUTES]) {
        throw new Error('Chutes API key is not set.');
    }

    if (multimodalApi === 'zai' && !secret_state[SECRET_KEYS.ZAI]) {
        throw new Error('Z.AI API key is not set.');
    }

    if (multimodalApi === 'pollinations' && oai_settings.pollinations_endpoint !== POLLINATIONS_ENDPOINT.ANONYMOUS && !secret_state[SECRET_KEYS.POLLINATIONS]) {
        throw new Error('Pollinations API key is not set.');
    }

    if (multimodalApi === 'workers_ai' && (!secret_state[SECRET_KEYS.WORKERS_AI] || !oai_settings.workers_ai_account_id)) {
        throw new Error('Workers AI API key or account ID is not set.');
    }
}

/**
 * Check if the WebLLM extension is installed and supported.
 * @returns {boolean} Whether the extension is installed and supported
 */
export function isWebLlmSupported() {
    if (!('gpu' in navigator)) {
        const warningKey = 'webllm_browser_warning_shown';
        if (!sessionStorage.getItem(warningKey)) {
            toastr.error('Your browser does not support the WebGPU API. Please use a different browser.', 'WebLLM', {
                preventDuplicates: true,
                timeOut: 0,
                extendedTimeOut: 0,
            });
            sessionStorage.setItem(warningKey, '1');
        }
        return false;
    }

    if (!('llm' in SillyTavern)) {
        const warningKey = 'webllm_extension_warning_shown';
        if (!sessionStorage.getItem(warningKey)) {
            toastr.error('WebLLM extension is not installed. Click here to install it.', 'WebLLM', {
                timeOut: 0,
                extendedTimeOut: 0,
                preventDuplicates: true,
                onclick: () => openThirdPartyExtensionMenu('https://github.com/SillyTavern/Extension-WebLLM'),
            });
            sessionStorage.setItem(warningKey, '1');
        }
        return false;
    }

    return true;
}

/**
 * Generates text in response to a chat prompt using WebLLM.
 * @param {any[]} messages Messages to use for generating
 * @param {object} params Additional parameters
 * @returns {Promise<string>} Generated response
 */
export async function generateWebLlmChatPrompt(messages, params = {}) {
    if (!isWebLlmSupported()) {
        throw new Error('WebLLM extension is not installed.');
    }

    console.debug('WebLLM chat completion request:', messages, params);
    const engine = SillyTavern.llm;
    const response = await engine.generateChatPrompt(messages, params);
    console.debug('WebLLM chat completion response:', response);
    return response;
}

/**
 * Counts the number of tokens in the provided text using WebLLM's default model.
 * Fallbacks to the current model's tokenizer if WebLLM token count fails.
 * @param {string} text Text to count tokens in
 * @returns {Promise<number>} Number of tokens in the text
 */
export async function countWebLlmTokens(text) {
    if (!isWebLlmSupported()) {
        throw new Error('WebLLM extension is not installed.');
    }

    try {
        const engine = SillyTavern.llm;
        const response = await engine.countTokens(text);
        return response;
    } catch (error) {
        // Fallback to using current model's tokenizer
        return await getTokenCountAsync(text);
    }
}

/**
 * Gets the size of the context in the WebLLM's default model.
 * @returns {Promise<number>} Size of the context in the WebLLM model
 */
export async function getWebLlmContextSize() {
    if (!isWebLlmSupported()) {
        throw new Error('WebLLM extension is not installed.');
    }

    const engine = SillyTavern.llm;
    await engine.loadModel();
    const model = await engine.getCurrentModelInfo();
    return model?.context_size;
}

/**
 * Request id of the item selected in Connection Manager: a profile id, a saved-model
 * id, or null.
 * @returns {string|null}
 */
export function getSelectedConnectionItemId() {
    const { selectedItem, selectedProfile } = SillyTavern.getContext().extensionSettings.connectionManager;
    if (selectedItem?.kind === MODEL_TARGET_SELECTION_KIND) {
        return `${MODEL_TARGET_ID_PREFIX}${selectedItem.id}`;
    }
    return selectedProfile ?? null;
}

/**
 * Adapter hints a saved model carries → request fields (see Connection Manager).
 * @type {Readonly<Record<string, string>>}
 */
const MODEL_TARGET_REQUEST_FIELDS = Object.freeze({
    customIncludeHeaders: 'custom_include_headers',
    customIncludeBody: 'custom_include_body',
    customExcludeBody: 'custom_exclude_body',
});

/**
 * A saved model seen as a read-only profile, so every profile-based caller (requests,
 * validation, icons, dropdowns) handles it without a second code path. It has no preset.
 * @param {import('./connection-manager/index.js').LlmModelTarget} target Saved model
 * @returns {import('./connection-manager/index.js').ConnectionProfile & { kind: string, adapterHints: Record<string, string> }}
 */
export function modelTargetAsProfile(target) {
    return Object.freeze({
        id: `${MODEL_TARGET_ID_PREFIX}${target.id}`,
        kind: target.kind,
        name: target.name,
        mode: target.mode,
        api: target.api,
        model: target.model,
        proxy: target.proxy,
        'api-url': target['api-url'],
        'custom-api-format': target['custom-api-format'],
        'secret-id': target.secretRef?.id,
        'prompt-post-processing': target.adapterHints?.promptPostProcessing,
        adapterHints: structuredClone(target.adapterHints ?? {}),
    });
}

/**
 * Request fields a saved model adds beyond the profile fields.
 * @param {{ adapterHints?: Record<string, string> }} profile Profile or saved-model view
 * @returns {Record<string, unknown>}
 */
function getModelTargetRequestFields(profile) {
    const hints = profile.adapterHints;
    if (!hints) {
        return {};
    }
    const fields = {};
    for (const [hint, field] of Object.entries(MODEL_TARGET_REQUEST_FIELDS)) {
        if (Object.hasOwn(hints, hint)) {
            fields[field] = hints[hint];
        }
    }
    fields.custom_claude_prompt_caching = hints.claudePromptCaching === 'enabled';
    fields.custom_openai_responses_websocket = hints.openaiResponsesMode === 'websocket';
    return fields;
}

/**
 * It uses the profiles to send a generate request to the API.
 */
export class ConnectionManagerRequestService {
    static defaultSendRequestParams = {
        stream: false,
        signal: null,
        extractData: true,
        includePreset: true,
        includeInstruct: true,
        instructSettings: {},
    };

    static getAllowedTypes() {
        return {
            openai: t`Chat Completion`,
            textgenerationwebui: t`Text Completion`,
        };
    }

    /**
     * @param {string} profileId
     * @param {string | (import('../custom-request.js').ChatCompletionMessage & {ignoreInstruct?: boolean})[]} prompt
     * @param {number} maxTokens
     * @param {Object} custom
     * @param {boolean?} [custom.stream=false]
     * @param {AbortSignal?} [custom.signal]
     * @param {boolean?} [custom.extractData=true]
     * @param {boolean?} [custom.includePreset=true]
     * @param {boolean?} [custom.includeInstruct=true]
     * @param {Partial<InstructSettings>?} [custom.instructSettings] Override instruct settings
     * @param {Record<string, any>} [overridePayload] - Override payload for the request
     * @returns {Promise<import('../custom-request.js').ExtractedData | (() => AsyncGenerator<import('../custom-request.js').StreamResponse>)>} If not streaming, returns extracted data; if streaming, returns a function that creates an AsyncGenerator
     */
    static async sendRequest(profileId, prompt, maxTokens, custom = this.defaultSendRequestParams, overridePayload = {}) {
        const { stream, signal, extractData, includePreset, includeInstruct, instructSettings } = { ...this.defaultSendRequestParams, ...custom };

        const context = SillyTavern.getContext();
        if (context.extensionSettings.disabledExtensions.includes('connection-manager')) {
            throw new Error('Connection Manager is not available');
        }

        const profile = this.getProfile(profileId);
        const selectedApiMap = this.validateProfile(profile);

        try {
            switch (selectedApiMap.selected) {
                case 'openai': {
                    if (!selectedApiMap.source) {
                        throw new Error(`API type ${selectedApiMap.selected} does not support chat completions`);
                    }

                    const proxyPreset = proxies.find((p) => p.name === profile.proxy);

                    const messages = Array.isArray(prompt) ? prompt : [{ role: 'user', content: prompt }];
                    return await context.ChatCompletionService.processRequest({
                        stream,
                        messages,
                        max_tokens: maxTokens,
                        model: profile.model,
                        chat_completion_source: selectedApiMap.source,
                        custom_api_format: profile['custom-api-format'],
                        opencode_api_format: profile['custom-api-format'],
                        opencode_endpoint: profile['api-url'],
                        secret_id: profile['secret-id'],
                        custom_url: profile['api-url'],
                        vertexai_region: profile['api-url'],
                        zai_endpoint: profile['api-url'],
                        siliconflow_endpoint: profile['api-url'],
                        minimax_endpoint: profile['api-url'],
                        moonshot_endpoint: profile['api-url'],
                        pollinations_endpoint: profile['api-url'],
                        reverse_proxy: proxyPreset?.url,
                        proxy_password: proxyPreset?.password,
                        custom_prompt_post_processing: profile['prompt-post-processing'],
                        ...getModelTargetRequestFields(profile),
                        ...overridePayload,
                    }, {
                        presetName: includePreset ? profile.preset : undefined,
                    }, extractData, signal);
                }
                case 'textgenerationwebui': {
                    if (!selectedApiMap.type) {
                        throw new Error(`API type ${selectedApiMap.selected} does not support text completions`);
                    }
                    if (globalThis.__TAURI_RUNNING__ === true) {
                        throw new Error('Text Completion profiles are not supported by the native TauriTavern generation backend yet. Use a Chat Completion profile.');
                    }

                    return await context.TextCompletionService.processRequest({
                        stream,
                        prompt,
                        max_tokens: maxTokens,
                        model: profile.model,
                        api_type: selectedApiMap.type,
                        api_server: profile['api-url'],
                        secret_id: profile['secret-id'],
                        ...overridePayload,
                    }, {
                        instructName: includeInstruct ? profile.instruct : undefined,
                        presetName: includePreset ? profile.preset : undefined,
                        instructSettings: includeInstruct ? instructSettings : undefined,
                    }, extractData, signal);
                }
                default: {
                    throw new Error(`Unknown API type ${selectedApiMap.selected}`);
                }
            }
        } catch (error) {
            throw new Error('API request failed', { cause: error });
        }
    }

    /**
    * If using text completion, return a formatted prompt string given an array of messages, a given profile ID, and optional instruct settings.
    * If using chat completion, simply return the given prompt as-is.
    * @param {ChatCompletionMessage[]} prompt An array of prompt messages.
    * @param {string} profileId ID of a given connection profile (from which to infer a completion preset).
    * @param {InstructSettings} instructSettings optional instruct settings
    */
    static constructPrompt(prompt, profileId, instructSettings = null) {
        const context = SillyTavern.getContext();
        const profile = this.getProfile(profileId);
        const selectedApiMap = this.validateProfile(profile);
        const instructName = profile.instruct;

        switch (selectedApiMap.selected) {
            case 'openai': {
                if (!selectedApiMap.source) {
                    throw new Error(`API type ${selectedApiMap.selected} does not support chat completions`);
                }
                return prompt;
            }
            case 'textgenerationwebui': {
                if (!selectedApiMap.type) {
                    throw new Error(`API type ${selectedApiMap.selected} does not support text completions`);
                }
                return context.TextCompletionService.constructPrompt(prompt, instructName, instructSettings);
            }
            default: {
                throw new Error(`Unknown API type ${selectedApiMap.selected}`);
            }
        }
    }

    /**
     * Respects allowed types.
     * @param {object} [options]
     * @param {boolean} [options.includeModelTargets=false] Also return saved models, as read-only profile views
     * @returns {import('./connection-manager/index.js').ConnectionProfile[]}
     */
    static getSupportedProfiles({ includeModelTargets = false } = {}) {
        const context = SillyTavern.getContext();
        if (context.extensionSettings.disabledExtensions.includes('connection-manager')) {
            throw new Error('Connection Manager is not available');
        }

        const { profiles, modelTargets } = context.extensionSettings.connectionManager;
        const items = includeModelTargets ? [...profiles, ...modelTargets.map(modelTargetAsProfile)] : profiles;
        return items.filter((p) => this.isProfileSupported(p));
    }

    /**
     * Profile data for a profile ID or a saved-model ID (`modelTarget:<id>`), or null.
     * @param {string} profileId
     * @returns {import('./connection-manager/index.js').ConnectionProfile|null}
     */
    static findProfile(profileId) {
        const { profiles, modelTargets } = SillyTavern.getContext().extensionSettings.connectionManager;
        if (typeof profileId === 'string' && profileId.startsWith(MODEL_TARGET_ID_PREFIX)) {
            const targetId = profileId.slice(MODEL_TARGET_ID_PREFIX.length);
            const target = modelTargets.find((t) => t.id === targetId);
            return target ? modelTargetAsProfile(target) : null;
        }
        return profiles.find((p) => p.id === profileId) ?? null;
    }

    /**
     * Return profile data given the profile ID. Saved models (`modelTarget:<id>`) resolve
     * to a read-only profile view without a preset.
     * @param {string} profileId
     * @returns {import('./connection-manager/index.js').ConnectionProfile?} [profile]
     * @throws {Error}
     */
    static getProfile(profileId) {
        const profile = this.findProfile(profileId);
        if (!profile) throw new Error(`Profile not found (ID: ${profileId})`);
        return profile;
    }

    /**
     * Creates a model icon Image element for the given profile (or the currently selected profile).
     * Returns null if the profile is not found, has no API, or Connection Manager is unavailable.
     * @param {string} [profileId] - Profile ID. If omitted, uses the currently selected profile.
     * @returns {HTMLImageElement | null}
     */
    static getProfileIcon(profileId) {
        if ((SillyTavern.getContext()).extensionSettings.disabledExtensions.includes('connection-manager')) {
            return null;
        }

        const id = profileId ?? getSelectedConnectionItemId();
        if (!id) return null;

        try {
            const profile = this.getProfile(id);
            if (!profile?.api) return null;
            return createModelIcon(profile.api, profile.model);
        } catch {
            return null;
        }
    }

    /**
     * @param {import('./connection-manager/index.js').ConnectionProfile?} [profile]
     * @returns {boolean}
     */
    static isProfileSupported(profile) {
        if (!profile || !profile.api) {
            return false;
        }

        const apiMap = CONNECT_API_MAP[profile.api];
        if (!Object.hasOwn(this.getAllowedTypes(), apiMap.selected)) {
            return false;
        }

        // Some providers not need model, like koboldcpp. But I don't want to check by provider.
        switch (apiMap.selected) {
            case 'openai':
                return !!apiMap.source;
            case 'textgenerationwebui':
                return !!apiMap.type;
        }

        return false;
    }

    /**
     * @param {import('./connection-manager/index.js').ConnectionProfile?} [profile]
     * @return {import('../slash-commands.js').ConnectAPIMap}
     * @throws {Error}
     */
    static validateProfile(profile) {
        if (!profile) {
            throw new Error('Could not find profile.');
        }
        if (!profile.api) {
            throw new Error('Select a connection profile that has an API');
        }

        const context = SillyTavern.getContext();
        const selectedApiMap = context.CONNECT_API_MAP[profile.api];
        if (!selectedApiMap) {
            throw new Error(`Unknown API type ${profile.api}`);
        }
        if (!Object.hasOwn(this.getAllowedTypes(), selectedApiMap.selected)) {
            throw new Error(`API type ${selectedApiMap.selected} is not supported. Supported types: ${Object.values(this.getAllowedTypes()).join(', ')}`);
        }

        return selectedApiMap;
    }

    /**
     * Create profiles dropdown and updates select element accordingly. Use onChange, onCreate, unUpdate, onDelete callbacks for custom behaviour. e.g updating extension settings.
     * @param {string} selector
     * @param {string} initialSelectedProfileId
     * @param {(profile?: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} onChange - 3 cases. 1- When user selects new profile. 2- When user deletes selected profile. 3- When user updates selected profile.
     * @param {(profile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} onCreate
     * @param {(oldProfile: import('./connection-manager/index.js').ConnectionProfile, newProfile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} unUpdate
     * @param {(profile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} onDelete
     * @param {object} [options]
     * @param {boolean} [options.includeModelTargets=false] Also list saved models (ids `modelTarget:<id>`).
     *   Off by default: callers written for upstream may look stored ids up in the profile list.
     */
    static handleDropdown(
        selector,
        initialSelectedProfileId,
        onChange = () => { },
        onCreate = () => { },
        unUpdate = () => { },
        onDelete = () => { },
        { includeModelTargets = false } = {},
    ) {
        const context = SillyTavern.getContext();
        if (context.extensionSettings.disabledExtensions.includes('connection-manager')) {
            throw new Error('Connection Manager is not available');
        }

        /**
         * @type {JQuery<HTMLSelectElement>}
         */
        const dropdown = $(selector);

        if (!dropdown || !dropdown.length) {
            throw new Error(`Could not find dropdown with selector ${selector}`);
        }

        dropdown.empty();

        // Create default option using document.createElement
        const defaultOption = document.createElement('option');
        defaultOption.value = '';
        defaultOption.textContent = 'Select a Connection Profile';
        defaultOption.dataset.i18n = 'Select a Connection Profile';
        dropdown.append(defaultOption);

        const profiles = context.extensionSettings.connectionManager.profiles;

        // Create optgroups using document.createElement
        const groups = {};
        for (const [apiType, groupLabel] of Object.entries(this.getAllowedTypes())) {
            const optgroup = document.createElement('optgroup');
            optgroup.label = groupLabel;
            groups[apiType] = optgroup;
        }

        const sortedProfilesByGroup = {};
        for (const apiType of Object.keys(this.getAllowedTypes())) {
            sortedProfilesByGroup[apiType] = [];
        }

        for (const profile of profiles) {
            if (this.isProfileSupported(profile)) {
                const apiMap = CONNECT_API_MAP[profile.api];
                if (sortedProfilesByGroup[apiMap.selected]) {
                    sortedProfilesByGroup[apiMap.selected].push(profile);
                }
            }
        }

        // Sort each group alphabetically and add to dropdown
        for (const [apiType, groupProfiles] of Object.entries(sortedProfilesByGroup)) {
            if (groupProfiles.length === 0) continue;

            groupProfiles.sort((a, b) => a.name.localeCompare(b.name));

            const group = groups[apiType];
            for (const profile of groupProfiles) {
                const option = document.createElement('option');
                option.value = profile.id;
                option.textContent = profile.name;
                group.appendChild(option);
            }
        }

        for (const group of Object.values(groups)) {
            if (group.children.length > 0) {
                dropdown.append(group);
            }
        }

        if (includeModelTargets) {
            this.#mirrorModelTargets(dropdown, defaultOption, { onCreate, onUpdate: unUpdate, onDelete });
        }

        const selectedProfile = includeModelTargets
            ? this.findProfile(initialSelectedProfileId)
            : profiles.find((p) => p.id === initialSelectedProfileId);
        if (selectedProfile) {
            dropdown.val(selectedProfile.id);
        }

        context.eventSource.on(context.eventTypes.CONNECTION_PROFILE_CREATED, async (profile) => {
            const isSupported = this.isProfileSupported(profile);
            if (!isSupported) {
                return;
            }

            const group = groups[CONNECT_API_MAP[profile.api].selected];
            const option = document.createElement('option');
            option.value = profile.id;
            option.textContent = profile.name;
            group.appendChild(option);

            await onCreate(profile);
        });

        context.eventSource.on(context.eventTypes.CONNECTION_PROFILE_UPDATED, async (oldProfile, newProfile) => {
            const currentSelected = dropdown.val();
            const isSelectedProfile = currentSelected === oldProfile.id;
            await unUpdate(oldProfile, newProfile);

            if (!this.isProfileSupported(newProfile)) {
                if (isSelectedProfile) {
                    dropdown.val('');
                    dropdown.trigger('change');
                }
                return;
            }

            const group = groups[CONNECT_API_MAP[newProfile.api].selected];
            const oldOption = group.querySelector(`option[value="${oldProfile.id}"]`);
            if (oldOption) {
                oldOption.remove();
            }

            const option = document.createElement('option');
            option.value = newProfile.id;
            option.textContent = newProfile.name;
            group.appendChild(option);

            if (isSelectedProfile) {
                // Ackchyually, we don't need to reselect but what if id changes? It is not possible for now I couldn't stop myself.
                dropdown.val(newProfile.id);
                dropdown.trigger('change');
            }
        });

        context.eventSource.on(context.eventTypes.CONNECTION_PROFILE_DELETED, async (profile) => {
            const currentSelected = dropdown.val();
            const isSelectedProfile = currentSelected === profile.id;
            if (!this.isProfileSupported(profile)) {
                return;
            }

            const group = groups[CONNECT_API_MAP[profile.api].selected];
            const optionToRemove = group.querySelector(`option[value="${profile.id}"]`);
            if (optionToRemove) {
                optionToRemove.remove();
            }

            if (isSelectedProfile) {
                dropdown.val('');
                dropdown.trigger('change');
            }

            await onDelete(profile);
        });

        dropdown.on('change', async () => {
            const profileId = String(dropdown.val() ?? '');
            const profile = this.findProfile(profileId) ?? undefined;
            await onChange(profile);
        });
    }

    /**
     * Keeps a "Models" group in an extension's profile dropdown, right after the
     * placeholder, in step with the saved models. Models go through the same callbacks
     * as profiles, as read-only profile views: `onCreate` / `onUpdate` / `onDelete` for
     * each supported model, and a selected model that is updated or deleted re-notifies
     * `onChange` (with no profile once deleted).
     * @param {JQuery<HTMLSelectElement>} dropdown Extension dropdown
     * @param {HTMLOptionElement} placeholder The leading "Select…" option
     * @param {object} callbacks The dropdown's profile callbacks
     * @param {(profile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} callbacks.onCreate
     * @param {(oldProfile: import('./connection-manager/index.js').ConnectionProfile, newProfile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} callbacks.onUpdate
     * @param {(profile: import('./connection-manager/index.js').ConnectionProfile) => Promise<void> | void} callbacks.onDelete
     */
    static #mirrorModelTargets(dropdown, placeholder, { onCreate, onUpdate, onDelete }) {
        const context = SillyTavern.getContext();
        const group = document.createElement('optgroup');
        group.label = t`Models`;
        /** @param {string} value */
        const hasOption = (value) => Array.from(dropdown.find('option')).some((option) => option.value === value);

        const render = () => {
            // Rebuilding the group drops its selected option; keep a selection that still exists.
            const selected = String(dropdown.val() ?? '');
            const targets = context.extensionSettings.connectionManager.modelTargets
                .map(modelTargetAsProfile)
                .filter((target) => this.isProfileSupported(target))
                .sort((a, b) => a.name.localeCompare(b.name));
            group.replaceChildren(...targets.map((target) => {
                const option = document.createElement('option');
                option.value = target.id;
                option.textContent = target.name;
                return option;
            }));
            // Removed rather than hidden: iOS WebKit ignores `hidden` on <optgroup>.
            if (targets.length === 0) {
                group.remove();
            } else if (!group.isConnected) {
                placeholder.after(group);
            }
            if (selected && hasOption(selected)) {
                dropdown.val(selected);
            }
        };
        render();

        context.eventSource.on(context.eventTypes.MODEL_TARGET_CREATED, async (target) => {
            render();
            const profile = modelTargetAsProfile(target);
            if (this.isProfileSupported(profile)) {
                await onCreate(profile);
            }
        });

        context.eventSource.on(context.eventTypes.MODEL_TARGET_UPDATED, async (oldTarget, newTarget) => {
            const oldProfile = modelTargetAsProfile(oldTarget);
            const newProfile = modelTargetAsProfile(newTarget);
            const isSelected = String(dropdown.val() ?? '') === oldProfile.id;
            await onUpdate(oldProfile, newProfile);
            render();
            if (isSelected) {
                // An update that made the model unsupported unselects it, as for profiles.
                dropdown.val(hasOption(newProfile.id) ? newProfile.id : '');
                dropdown.trigger('change');
            }
        });

        context.eventSource.on(context.eventTypes.MODEL_TARGET_DELETED, async (target) => {
            const profile = modelTargetAsProfile(target);
            const isSelected = String(dropdown.val() ?? '') === profile.id;
            render();
            if (!this.isProfileSupported(profile)) {
                return;
            }
            if (isSelected) {
                dropdown.val('');
                dropdown.trigger('change');
            }
            await onDelete(profile);
        });
    }
}
