// @ts-check

/**
 * Reasoning Effort values offered for a chat-completion connection; `auto` sends nothing.
 *
 * Native sources use the project vocabulary, which provider adapters map per
 * provider and model. Custom and OpenCode forward the value verbatim, so they
 * offer the wire vocabulary of the selected API format instead; a project level
 * stored there (e.g. by a SillyTavern preset) is mapped into that vocabulary.
 */
const PROJECT_EFFORTS = Object.freeze(['auto', 'min', 'low', 'medium', 'high', 'xhigh', 'max']);
const OPENAI_EFFORTS = Object.freeze(['auto', 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max']);
const CLAUDE_EFFORTS = Object.freeze(['auto', 'low', 'medium', 'high', 'xhigh', 'max']);
const GEMINI_LEVELS = Object.freeze(['auto', 'minimal', 'low', 'medium', 'high']);

/** @type {Readonly<Record<string, readonly string[]>>} */
const EFFORTS_BY_API_FORMAT = Object.freeze({
    openai_compat: OPENAI_EFFORTS,
    openai_responses: OPENAI_EFFORTS,
    claude_messages: CLAUDE_EFFORTS,
    gemini_interactions: GEMINI_LEVELS,
    gemini_generate_content: GEMINI_LEVELS,
    // OpenCode's name for Gemini generateContent.
    gemini: GEMINI_LEVELS,
});

/**
 * Project levels a format lacks, as SillyTavern maps them for the Custom source
 * (`min` → `low`, `max` → `high`). `xhigh` falls back to `high`, as on native
 * sources without it. Applied only when the format does not offer the level itself.
 * @type {Readonly<Record<string, string>>}
 */
const PROJECT_LEVEL_FALLBACKS = Object.freeze({ min: 'low', xhigh: 'high', max: 'high' });

/** @typedef {{ chat_completion_source?: unknown; custom_api_format?: unknown; opencode_api_format?: unknown; reasoning_effort?: unknown }} EffortSettings */

/**
 * @param {EffortSettings} settings
 * @returns {unknown} The API format whose vocabulary applies, or `null` for native sources.
 */
function apiFormatOf(settings) {
    return settings.chat_completion_source === 'custom'
        ? settings.custom_api_format
        : settings.chat_completion_source === 'opencode'
            ? settings.opencode_api_format
            : null;
}

/**
 * @param {EffortSettings} settings
 * @returns {readonly string[]}
 */
export function getReasoningEffortOptions(settings) {
    const format = apiFormatOf(settings);
    if (format === null) {
        return PROJECT_EFFORTS;
    }
    const key = String(format || 'openai_compat');
    const options = Object.hasOwn(EFFORTS_BY_API_FORMAT, key) ? EFFORTS_BY_API_FORMAT[key] : undefined;
    if (!options) {
        throw new Error(`Unknown ${settings.chat_completion_source} API format for Reasoning Effort: ${key}`);
    }
    return options;
}

/**
 * The Reasoning Effort a request on these settings uses: the stored value when the
 * connection offers it, a project level mapped into a Custom / OpenCode format, otherwise
 * `auto`. The stored value is never rewritten, so a value from another source / format
 * applies again once that connection is selected.
 * @param {EffortSettings} settings
 * @returns {string}
 */
export function getEffectiveReasoningEffort(settings) {
    const value = settings.reasoning_effort;
    if (typeof value !== 'string') {
        return 'auto';
    }
    const options = getReasoningEffortOptions(settings);
    if (options.includes(value)) {
        return value;
    }
    // Native sources offer every project level, so only Custom / OpenCode formats reach this.
    const fallback = Object.hasOwn(PROJECT_LEVEL_FALLBACKS, value) ? PROJECT_LEVEL_FALLBACKS[value] : undefined;
    return fallback && options.includes(fallback) ? fallback : 'auto';
}
