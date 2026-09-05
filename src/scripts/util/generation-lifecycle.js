/**
 * Determines whether an unhandled Generate() error left the foreground
 * generation UI in a locked state that still needs the legacy unblock path.
 *
 * @param {{
 *   dryRun: boolean;
 *   isSendPress: boolean;
 *   isBodyGenerating: boolean;
 *   isGroupGenerating: boolean;
 * }} state
 * @returns {boolean}
 */
export function shouldUnblockGenerationAfterUnhandledError(state) {
    if (state.dryRun || state.isGroupGenerating) {
        return false;
    }

    return Boolean(state.isSendPress || state.isBodyGenerating);
}

/**
 * Legacy character events describe visible Assistant content. A first-class
 * Assistant that only owns tool calls is published through the tool events.
 *
 * @param {ChatMessage} message
 * @param {boolean} hasToolCalls
 * @returns {boolean}
 */
export function shouldEmitCharacterMessageEvents(message, hasToolCalls) {
    if (!hasToolCalls) {
        return true;
    }

    const text = message.mes.trim();
    const reasoning = message.extra?.reasoning?.trim() ?? '';
    const hasMedia = Boolean(message.extra?.media?.length);
    return !['', '...'].includes(text) || Boolean(reasoning) || hasMedia;
}
