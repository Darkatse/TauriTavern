/** Build a credential-free preview matching the Custom provider URL builders. */
export function getCustomEndpointPreview(settings) {
    const base = String(settings.custom_url || '').trim().replace(/\/+$/, '') || '<Base URL>';
    let suffix;
    switch (settings.custom_api_format) {
        case 'gemini_generate_content': {
            const model = String(settings.custom_model || '').trim() || '<Model>';
            const modelPath = model.startsWith('models/') ? model : `models/${model}`;
            const version = /\/v1(?:beta)?$/.test(base) ? '' : '/v1beta';
            const method = settings.stream_openai ? 'streamGenerateContent?alt=sse' : 'generateContent';
            suffix = `${version}/${modelPath}:${method}`;
            break;
        }
        case 'gemini_interactions':
            suffix = '/interactions';
            break;
        case 'claude_messages':
            suffix = '/messages';
            break;
        case 'openai_responses':
            suffix = '/responses';
            break;
        default:
            suffix = '/chat/completions';
    }
    return { suffix, url: `${base}${suffix}` };
}
