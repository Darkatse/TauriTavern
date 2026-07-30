const OPENAI_MESSAGE_TO_TEXT_OFFSET = 1;
const OPENAI_NON_FULL_CONVERSATION_OFFSET = 2;

/** Converts a raw single-message count into its caller-visible text count. */
export function getOpenAITextTokenCount(messageTokenCount) {
    return Number(messageTokenCount) - OPENAI_MESSAGE_TO_TEXT_OFFSET;
}

export function getOpenAIConversationTokenCount(messageTokenCounts, full = false) {
    const tokenCount = messageTokenCounts.reduce(
        (total, count) => total + Number(count),
        -OPENAI_MESSAGE_TO_TEXT_OFFSET,
    );

    return full ? tokenCount : tokenCount - OPENAI_NON_FULL_CONVERSATION_OFFSET;
}

/** Compares a raw single-message count with a caller-visible text threshold. */
export function hasReachedOpenAITextTokenLimit(messageTokenCount, stopAt) {
    return Number.isFinite(stopAt) && getOpenAITextTokenCount(messageTokenCount) >= stopAt;
}
