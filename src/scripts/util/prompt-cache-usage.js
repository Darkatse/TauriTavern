/**
 * Read prompt cache usage from normalized completions or native Claude/Gemini streams.
 * Missing cache counters mean unknown; an explicit zero means a cache miss.
 * @param {object} usage Provider usage snapshot
 * @returns {{ input_tokens: number, cached_tokens: number } | null}
 */
export function getPromptCacheUsage(usage) {
    const cachedTokens = usage?.prompt_tokens_details?.cached_tokens
        ?? usage?.cached_tokens
        ?? usage?.prompt_cache_hit_tokens
        ?? usage?.cache_read_input_tokens
        ?? usage?.cachedContentTokenCount;
    if (cachedTokens == null) return null;

    // Claude's input_tokens excludes both cache reads and cache writes.
    const inputTokens = usage.input_tokens != null
        ? usage.input_tokens + (usage.cache_creation_input_tokens ?? 0) + cachedTokens
        : usage.prompt_tokens ?? usage.promptTokenCount;
    if (inputTokens == null) return null;

    if (!Number.isSafeInteger(inputTokens) || !Number.isSafeInteger(cachedTokens)
        || cachedTokens < 0 || inputTokens < cachedTokens) {
        throw new Error('Invalid prompt cache usage: expected 0 <= cached tokens <= input tokens');
    }
    return { input_tokens: inputTokens, cached_tokens: cachedTokens };
}
