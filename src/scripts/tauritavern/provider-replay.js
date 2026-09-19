/** The response parser and saved provider artifacts use the request's connection. */
export function getChatCompletionRequestContext(settings, model = settings.model) {
    return {
        mainApi: 'openai',
        chatCompletionSource: settings.chat_completion_source ?? 'openai',
        model: model ?? '',
        customApiFormat: settings.custom_api_format ?? 'openai_compat',
        opencodeApiFormat: settings.opencode_api_format ?? 'openai_compat',
    };
}

/** Compare canonical text before prompt formatting; never bind a continuation to its combined text. */
export function canReplayProviderMetadata(metadataMessage, text, context) {
    const extra = metadataMessage?.extra;
    const replay = extra?.provider_replay;
    if (replay) {
        return replay.text === text
            && replay.chatCompletionSource === context.chatCompletionSource
            && replay.model === context.model
            && (context.chatCompletionSource !== 'custom' || replay.customApiFormat === context.customApiFormat)
            && (context.chatCompletionSource !== 'opencode' || replay.opencodeApiFormat === context.opencodeApiFormat);
    }

    // Older records can be verified when their native payload still contains the original body.
    const native = extra?.native;
    const format = native?.gemini ? 'gemini_generate_content'
        : native?.gemini_interactions ? 'gemini_interactions'
            : native?.claude ? 'claude_messages' : null;
    if (!format || extra.api !== context.chatCompletionSource || extra.model !== context.model
        || (context.chatCompletionSource === 'custom' && format !== context.customApiFormat)
        || (context.chatCompletionSource === 'opencode' && format !== context.opencodeApiFormat)) {
        return false;
    }
    if (native.gemini) {
        return native.gemini.content?.parts?.filter(part => !part.thought && typeof part.text === 'string').map(part => part.text).join('\n\n') === text;
    }
    if (native.claude) {
        return native.claude.content?.filter(part => part.type === 'text').map(part => part.text).join('\n\n') === text;
    }
    return native.gemini_interactions.steps?.filter(step => step.type === 'model_output')
        .flatMap(step => step.content).filter(part => part.type === 'text').map(part => part.text).join('') === text;
}
