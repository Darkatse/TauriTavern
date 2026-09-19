import { createAbortError, isAbortError } from '../kernel/abort-error.js';
import {
    buildLegacyErrorPayload,
    getErrorMessage,
} from './ai-error-presenter.js';

const CAPTION_UNAVAILABLE_MESSAGE = 'Image captioning is not implemented in the TauriTavern native backend.';
const UNAVAILABLE_CAPTION_ROUTES = Object.freeze([
    '/api/extra/caption',
    '/api/horde/caption-image',
    '/api/backends/text-completions/ollama/caption-image',
]);
const CAPTION_SOURCES = Object.freeze({
    openai: 'openai',
    openrouter: 'openrouter',
    custom: 'custom',
    anthropic: 'claude',
    google: 'makersuite',
    vertexai: 'vertexai',
    cohere: 'cohere',
    groq: 'groq',
    moonshot: 'moonshot',
    nanogpt: 'nanogpt',
    chutes: 'chutes',
    workers_ai: 'workers_ai',
    zai: 'zai',
});

class CaptionRequestError extends Error {
    constructor(status, message) {
        super(message);
        this.status = status;
    }
}

function asObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function requiredString(payload, key) {
    const value = payload[key];
    if (typeof value !== 'string' || !value.trim()) {
        throw new CaptionRequestError(400, `Image captioning request requires a non-empty ${key}.`);
    }
    return value.trim();
}

function optionalString(payload, key) {
    const value = payload[key];
    if (value === undefined || value === null) {
        return '';
    }
    if (typeof value !== 'string') {
        throw new CaptionRequestError(400, `Image captioning request field must be a string: ${key}.`);
    }
    return value.trim();
}

function captionRouteFor(api) {
    if (api === 'anthropic') {
        return 'anthropic';
    }
    if (api === 'google' || api === 'vertexai') {
        return 'google';
    }
    return 'openai';
}

function parseMedia(image) {
    const separator = image.indexOf(',');
    const header = separator >= 0 ? image.slice(0, separator) : '';
    const match = /^data:(image|video)\/[^;,]+;base64$/i.exec(header);
    if (!match || !image.slice(separator + 1).trim()) {
        throw new CaptionRequestError(400, 'Image captioning requires a base64 image or video data URL.');
    }
    return match[1].toLowerCase();
}

function buildCaptionPayload(body, route) {
    const payload = asObject(body);
    const api = optionalString(payload, 'api').toLowerCase() || route;
    const source = Object.prototype.hasOwnProperty.call(CAPTION_SOURCES, api)
        ? CAPTION_SOURCES[api]
        : '';
    if (!source) {
        throw new CaptionRequestError(501, `Image captioning via "${api}" is not implemented in the TauriTavern native backend.`);
    }
    if (captionRouteFor(api) !== route) {
        throw new CaptionRequestError(400, `Image captioning API "${api}" does not match this endpoint.`);
    }

    const image = requiredString(payload, 'image');
    const model = requiredString(payload, 'model');
    const prompt = payload.prompt;
    if (typeof prompt !== 'string') {
        throw new CaptionRequestError(400, 'Image captioning request field must be a string: prompt.');
    }

    const mediaKind = parseMedia(image);
    if (mediaKind === 'video' && !['google', 'vertexai', 'zai'].includes(api)) {
        throw new CaptionRequestError(400, `Image captioning via "${api}" does not support video input.`);
    }

    const media = mediaKind === 'video'
        ? { type: 'video_url', video_url: { url: image } }
        : { type: 'image_url', image_url: { url: image } };
    const text = { type: 'text', text: prompt };
    const dto = {
        chat_completion_source: source,
        model,
        type: 'quiet',
        stream: false,
        messages: [{
            role: 'user',
            content: api === 'anthropic' ? [media, text] : [text, media],
        }],
        reverse_proxy: optionalString(payload, 'reverse_proxy'),
        proxy_password: optionalString(payload, 'proxy_password'),
    };

    if (api === 'custom') {
        dto.custom_url = requiredString(payload, 'server_url');
        dto.custom_api_format = 'openai_compat';
        dto.custom_include_headers = payload.custom_include_headers ?? null;
        dto.custom_include_body = payload.custom_include_body ?? null;
        dto.custom_exclude_body = payload.custom_exclude_body ?? null;
    } else if (api === 'vertexai') {
        dto.vertexai_auth_mode = optionalString(payload, 'vertexai_auth_mode');
        dto.vertexai_region = optionalString(payload, 'vertexai_region');
        dto.vertexai_express_project_id = optionalString(payload, 'vertexai_express_project_id');
    } else if (api === 'zai') {
        dto.zai_endpoint = optionalString(payload, 'zai_endpoint');
        dto.max_tokens = 4096;
    } else if (api === 'workers_ai') {
        dto.workers_ai_account_id = optionalString(payload, 'workers_ai_account_id');
    } else if (api === 'moonshot') {
        dto.moonshot_endpoint = optionalString(payload, 'moonshot_endpoint');
    } else if (api === 'anthropic') {
        dto.max_tokens = 4096;
    }

    return dto;
}

function captionFromCompletion(completion) {
    const caption = completion?.choices?.[0]?.message?.content;
    if (typeof caption !== 'string' || !caption.trim()) {
        throw new Error('Image captioning response is missing assistant text.');
    }
    return caption.trim();
}

export function registerCaptionRoutes(router, { jsonResponse, invokeChatCompletion }) {
    for (const route of UNAVAILABLE_CAPTION_ROUTES) {
        router.post(route, async () => jsonResponse({
            error: true,
            message: CAPTION_UNAVAILABLE_MESSAGE,
        }, 501));
    }

    function register(path, route) {
        router.post(path, async ({ body, init }) => {
            try {
                const dto = buildCaptionPayload(body, route);
                const completion = await invokeChatCompletion(dto, init?.signal);
                return jsonResponse({ caption: captionFromCompletion(completion) });
            } catch (error) {
                if (error instanceof CaptionRequestError) {
                    return jsonResponse({ error: true, message: error.message }, error.status);
                }
                if (isAbortError(error) || /generation cancelled by user/i.test(getErrorMessage(error))) {
                    throw createAbortError();
                }
                console.error('Image captioning failed:', error);
                return jsonResponse(buildLegacyErrorPayload(error), 502);
            }
        });
    }

    register('/api/openai/caption-image', 'openai');
    register('/api/google/caption-image', 'google');
    register('/api/anthropic/caption-image', 'anthropic');
}
