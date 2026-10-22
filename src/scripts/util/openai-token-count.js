const OPENAI_NON_FULL_CONVERSATION_OFFSET = 2;

export function getOpenAIConversationTokenCount(messageTokenCounts, full = false) {
    const tokenCount = messageTokenCounts.reduce(
        (total, count) => total + Number(count),
        0,
    );

    return full ? tokenCount : tokenCount - OPENAI_NON_FULL_CONVERSATION_OFFSET;
}
