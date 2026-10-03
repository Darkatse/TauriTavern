// @ts-check

/**
 * Static knowledge the generation-params panel cannot derive at runtime.
 *
 * Everything else — which controls exist, which source supports them, their
 * labels and value types — comes from upstream `settingsToUpdate` plus the
 * DOM, so new upstream settings appear in the panel without edits here.
 *
 * Kinds, decided per `settingsToUpdate` entry:
 * - `toggle`: checkbox. Visible ⇔ enabled; nothing extra is stored.
 * - `request`: preset key listed in `PAYLOAD_KEYS`. Removing omits the payload
 *   key from the outgoing request (server default applies); the value is kept.
 * - `local`: anything else. Removing only hides the block on this device; the
 *   value keeps applying. This is the safe default for unknown upstream keys.
 * - `fallback`: preset key listed in `FALLBACK_VALUES`. Always applies a
 *   value; removing resets it to the fallback.
 */

/**
 * Preset key (`settingsToUpdate` key) → key in `createGenerationParameters()`
 * output. Only these may be omitted, so a foreign preset can never strip
 * structural fields such as `messages` or `model`. Extend when upstream adds a
 * setting that becomes a payload field.
 * @type {Readonly<Record<string, string>>}
 */
export const PAYLOAD_KEYS = Object.freeze({
    temperature: 'temperature',
    frequency_penalty: 'frequency_penalty',
    presence_penalty: 'presence_penalty',
    top_p: 'top_p',
    top_k: 'top_k',
    top_a: 'top_a',
    min_p: 'min_p',
    repetition_penalty: 'repetition_penalty',
    seed: 'seed',
    n: 'n',
    openai_max_tokens: 'max_tokens',
    reasoning_effort: 'reasoning_effort',
    verbosity: 'verbosity',
    openrouter_middleout: 'middleout',
    assistant_prefill: 'assistant_prefill',
});

/** @type {ReadonlySet<string>} */
export const REQUEST_PARAM_KEYS = new Set(Object.values(PAYLOAD_KEYS));

/**
 * Preset keys shown above the foldable section and never removable: streaming
 * keeps its checkbox, and reasoning effort already has Auto for "not sent".
 */
export const PINNED_PRESET_KEYS = Object.freeze(['stream_openai', 'reasoning_effort']);

/**
 * Local-only preset keys that always apply a value (kind `fallback`). Removing
 * one resets it to this value and hides it; any other value shows it again.
 * @type {Readonly<Record<string, number>>}
 */
export const FALLBACK_VALUES = Object.freeze({
    openai_max_context: 1_000_000,
});

/**
 * Reply tokens reserved in the local prompt budget while `max_tokens` is
 * omitted. Formats that require the field fill their own value in the backend.
 */
export const OMITTED_MAX_TOKENS_BUDGET = 25_000;

/**
 * Payload keys that upstream renames per model; omitting the catalog key strips them too.
 * @type {Readonly<Record<string, readonly string[]>>}
 */
export const PAYLOAD_KEY_ALIASES = Object.freeze({
    max_tokens: ['max_completion_tokens'],
});

/**
 * One-line help per block, keyed by the block's param key (see `panel.js`).
 * English source strings; translated through the locale files.
 * @type {Readonly<Record<string, string>>}
 */
export const PARAM_HINTS = Object.freeze({
    openai_max_context: 'Token budget for prompt plus reply; older chat is cut to fit.',
    max_tokens: 'Most tokens the model may generate in one reply.',
    stream_openai: 'Show the reply while it is being generated.',
    n: 'Number of replies generated per request.',
    middleout: 'Let OpenRouter cut the middle of a prompt that is too long.',
    temperature: 'Higher values make the output more random.',
    frequency_penalty: 'Discourages words the more often they have appeared.',
    presence_penalty: 'Discourages words that have already appeared.',
    top_k: 'Sample only from the K most likely tokens; 0 means no limit.',
    top_p: 'Sample only from the most likely tokens whose probabilities add up to P.',
    repetition_penalty: 'Discourages repetition; 1 turns it off.',
    min_p: 'Drop tokens less likely than P times the most likely token.',
    top_a: 'Drop tokens less likely than A times the square of the top probability.',
    quick_prompts: 'Edit the main prompts right here.',
    utility_prompts: 'Prompts used by helper features such as impersonation and continue.',
    seed: 'A fixed seed makes results repeatable; -1 is random.',
    names_behavior: 'How character names are added to messages.',
    continue_postfix: 'Text placed between the reply and its continuation.',
    continue_prefill: 'Continue by sending the last reply as an assistant prefill.',
    squash_system_messages: 'Merge consecutive system messages into one.',
    use_sysprompt: 'Send the system prompt as a system message.',
    claude_fast_mode: 'Faster output at a higher price.',
    custom_claude_adaptive_thinking: 'Let Claude decide whether and how much to think.',
    custom_responses_reasoning_summary: 'Ask the model for a summary of its reasoning.',
    enable_web_search: 'Let the model search the web.',
    function_calling: 'Let the model call tools.',
    tool_reasoning_mode: 'How earlier reasoning is sent back with tool calls.',
    media_inlining: 'Send images and other media in the prompt.',
    request_images: 'Let the model return images.',
    show_thoughts: 'Show the model\'s reasoning when it is returned.',
    reasoning_effort: 'How much the model thinks before replying.',
    verbosity: 'How long and detailed the reply is.',
    assistant_prefill: 'Text the reply is forced to start with.',
    bias_preset_selected: 'Make specific tokens more or less likely.',
});

/** Upstream containers whose settings the panel manages. */
export const PANEL_SCOPE = '#range_block_openai, #openai_settings';

/**
 * A block supported by at most this many sources is a provider-specific
 * feature (Middle-out, Assistant Prefill, Gemini image output…) and is
 * grouped under the current source; everything else is a general parameter
 * that merely lacks support on some providers. Support itself is still
 * decided by upstream's `[data-source]` visibility.
 */
export const SOURCE_SPECIFIC_MAX_SOURCES = 3;

/**
 * Prompt drawers that are not settings entries; managed as `local`.
 * @type {ReadonlyArray<{ key: string, controlId: string }>}
 */
export const DRAWERS = Object.freeze([
    { key: 'quick_prompts', controlId: 'main_prompt_quick_edit_textarea' },
    { key: 'utility_prompts', controlId: 'impersonation_prompt_textarea' },
]);
