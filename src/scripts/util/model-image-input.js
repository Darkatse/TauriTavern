/** @type {ReadonlyMap<string, (model: any) => unknown>} */
const IMAGE_INPUT_MARKERS = new Map([
    ['aimlapi', model => model.features?.includes('openai/chat-completion.vision')],
    ['chutes', model => model.input_modalities?.includes('image')],
    ['electronhub', model => model.metadata?.vision],
    ['fireworks', model => model.supports_image_input],
    ['mistralai', model => model.capabilities?.vision],
    ['moonshot', model => model.supports_image_in],
    ['nanogpt', model => model.capabilities?.vision],
    ['openrouter', model => model.architecture?.input_modalities?.includes('image')],
    ['pollinations', model => model.input_modalities?.includes('image')],
    ['workers_ai', model => model.properties?.some(property => property.property_id === 'vision' && property.value === 'true')],
    ['xai', model => model.input_modalities?.includes('image')],
]);

/**
 * @param {string} source
 * @param {any} model
 * @returns {boolean}
 */
export function modelAcceptsImages(source, model) {
    const marker = IMAGE_INPUT_MARKERS.get(source);
    return Boolean(marker && model && marker(model));
}
