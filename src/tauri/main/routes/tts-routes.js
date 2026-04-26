import { textResponse } from '../http-utils.js';

const SECRET_KEYS = Object.freeze({
    MIMO: 'api_key_mimo',
    XAI: 'api_key_xai',
});

const XAI_VOICES = new Set(['ara', 'eve', 'leo', 'rex', 'sal']);
const MIMO_MODELS = new Set(['mimo-v2-tts', 'mimo-v2.5-tts']);
const MIMO_FORMATS = new Set(['wav', 'mp3']);

function getSecretValue(secretResult) {
    return String(secretResult?.value || '').trim();
}

async function findSecret(context, key) {
    return await context.safeInvoke('find_secret', { dto: { key } });
}

function errorTextResponse(message, status = 400) {
    const normalized = String(message || '').trim() || 'Request failed';
    return textResponse(normalized, status, normalized);
}

function sanitizeText(value) {
    return typeof value === 'string' ? value : '';
}

function parseUpstreamErrorMessage(payload, fallback) {
    if (!payload) {
        return fallback;
    }

    if (typeof payload === 'string') {
        return payload || fallback;
    }

    if (typeof payload.error === 'string') {
        return payload.error || fallback;
    }

    if (typeof payload.message === 'string') {
        return payload.message || fallback;
    }

    if (payload.error && typeof payload.error === 'object') {
        if (typeof payload.error.message === 'string') {
            return payload.error.message || fallback;
        }
    }

    return fallback;
}

async function buildUpstreamErrorResponse(response, fallback) {
    const status = Number(response?.status) || 502;

    try {
        const payload = await response.json();
        return errorTextResponse(parseUpstreamErrorMessage(payload, fallback), status);
    } catch {
        try {
            return errorTextResponse(await response.text(), status);
        } catch {
            return errorTextResponse(fallback, status);
        }
    }
}

function decodeBase64Audio(base64Audio) {
    const binary = atob(base64Audio);
    const bytes = new Uint8Array(binary.length);

    for (let index = 0; index < binary.length; index++) {
        bytes[index] = binary.charCodeAt(index);
    }

    return bytes;
}

function getMimoContentType(format) {
    switch (String(format || '').toLowerCase()) {
        case 'mp3':
            return 'audio/mpeg';
        case 'wav':
        default:
            return 'audio/wav';
    }
}

export function registerTtsRoutes(router, context) {
    router.post('/api/tts/grok/generate', async ({ body, init }) => {
        try {
            const apiKey = getSecretValue(await findSecret(context, SECRET_KEYS.XAI));
            if (!apiKey) {
                return errorTextResponse('xAI API key is required', 400);
            }

            const text = sanitizeText(body?.text).trim();
            const voiceId = String(body?.voiceId || 'eve').trim().toLowerCase();
            const language = String(body?.language || 'auto').trim() || 'auto';
            const outputFormat = body?.outputFormat && typeof body.outputFormat === 'object' ? body.outputFormat : {};

            if (!text) {
                return errorTextResponse('No text provided', 400);
            }

            if (!XAI_VOICES.has(voiceId)) {
                return errorTextResponse(`Unsupported Grok voice: ${voiceId}`, 400);
            }

            const response = await fetch('https://api.x.ai/v1/tts', {
                method: 'POST',
                signal: init?.signal,
                headers: {
                    Authorization: `Bearer ${apiKey}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    text,
                    voice_id: voiceId,
                    language,
                    output_format: {
                        codec: String(outputFormat.codec || 'mp3'),
                        sample_rate: Number(outputFormat.sampleRate) || 24000,
                        bit_rate: Number(outputFormat.bitRate) || 128000,
                    },
                }),
            });

            if (!response.ok) {
                return await buildUpstreamErrorResponse(response, 'Grok TTS request failed');
            }

            const contentType = response.headers.get('content-type') || 'audio/mpeg';
            const buffer = await response.arrayBuffer();
            return new Response(buffer, {
                status: 200,
                headers: {
                    'Content-Type': contentType,
                },
            });
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error || 'Unknown error');
            return errorTextResponse(`Grok TTS request failed: ${message}`, 502);
        }
    });

    router.post('/api/tts/mimo/generate', async ({ body, init }) => {
        try {
            const apiKey = getSecretValue(await findSecret(context, SECRET_KEYS.MIMO));
            if (!apiKey) {
                return errorTextResponse('MiMo API key is required', 400);
            }

            const text = sanitizeText(body?.text).trim();
            const voiceId = String(body?.voiceId || 'mimo_default').trim() || 'mimo_default';
            const model = String(body?.model || 'mimo-v2-tts').trim() || 'mimo-v2-tts';
            const format = String(body?.format || 'wav').trim().toLowerCase() || 'wav';
            const instructions = sanitizeText(body?.instructions).trim();

            if (!text) {
                return errorTextResponse('No text provided', 400);
            }

            if (!MIMO_MODELS.has(model)) {
                return errorTextResponse(`Unsupported MiMo model: ${model}`, 400);
            }

            if (!MIMO_FORMATS.has(format)) {
                return errorTextResponse(`Unsupported MiMo audio format: ${format}`, 400);
            }

            const messages = [];
            if (instructions) {
                messages.push({
                    role: 'user',
                    content: instructions,
                });
            }
            messages.push({
                role: 'assistant',
                content: text,
            });

            const response = await fetch('https://api.xiaomimimo.com/v1/chat/completions', {
                method: 'POST',
                signal: init?.signal,
                headers: {
                    'api-key': apiKey,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    model,
                    messages,
                    audio: {
                        format,
                        voice: voiceId,
                    },
                }),
            });

            if (!response.ok) {
                return await buildUpstreamErrorResponse(response, 'MiMo TTS request failed');
            }

            const payload = await response.json();
            const audioBase64 = payload?.choices?.[0]?.message?.audio?.data;

            if (typeof audioBase64 !== 'string' || !audioBase64) {
                return errorTextResponse('MiMo TTS response did not include audio data', 502);
            }

            const audioBytes = decodeBase64Audio(audioBase64);
            return new Response(audioBytes, {
                status: 200,
                headers: {
                    'Content-Type': getMimoContentType(format),
                },
            });
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error || 'Unknown error');
            return errorTextResponse(`MiMo TTS request failed: ${message}`, 502);
        }
    });
}
