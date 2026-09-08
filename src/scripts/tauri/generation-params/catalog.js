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
    reasoning_effort: 'reasoning_effort',
    verbosity: 'verbosity',
    openrouter_middleout: 'middleout',
    assistant_prefill: 'assistant_prefill',
});

/** @type {ReadonlySet<string>} */
export const REQUEST_PARAM_KEYS = new Set(Object.values(PAYLOAD_KEYS));

/** Preset keys inside the panel that belong to the header row and are never optional. */
export const EXCLUDED_PRESET_KEYS = new Set(['openai_max_context', 'max_context_unlocked', 'openai_max_tokens', 'stream_openai']);

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
