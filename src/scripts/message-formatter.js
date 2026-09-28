import { messageFormatting } from '../script.js';

/** Synchronous display transforms. Every stage precedes message sanitization. */
export const formatting_stage = {
    BEFORE_REGEX: 'beforeRegex',
    AFTER_REGEX: 'afterRegex',
    AFTER_MARKDOWN: 'afterMarkdown',
};

export const hook_order = {
    EARLIEST: 0,
    EARLY: 10,
    NORMAL: 50,
    LATE: 90,
    LATEST: 100,
};

/**
 * @typedef {object} MessageFormattingBase
 * @property {string} ch_name Character name (the upstream runtime field).
 * @property {boolean} isSystem Normalized system-message flag.
 * @property {boolean} isUser Whether this is a user message.
 * @property {number} messageId Chat index, or -1 for a standalone preview.
 * @property {boolean} isReasoning Whether this is reasoning text.
 */
/** @typedef {Readonly<MessageFormattingBase & { stage: string }>} MessageFormattingContext */
/** @typedef {(text: string, context: MessageFormattingContext) => string} MessageFormattingHook */

class MessageFormatterRegistry {
    /** @type {Map<string, { fn: MessageFormattingHook; order: number }[]>} */
    #hooks = new Map(Object.values(formatting_stage).map(stage => [stage, []]));

    stage = formatting_stage;
    order = hook_order;

    /**
     * Registers a display transform, in ascending order within its stage.
     * Formatting may run again for streaming, edits or remounts; this is not a
     * message lifecycle event. Registration affects subsequent formatting.
     * @param {MessageFormattingHook} fn
     * @param {{ stage?: string; order?: number }} [options]
     */
    addHook(fn, { stage = formatting_stage.AFTER_MARKDOWN, order = hook_order.NORMAL } = {}) {
        if (typeof fn !== 'function') throw new TypeError('MessageFormatter: hook must be a function');
        if (fn.constructor?.name === 'AsyncFunction') throw new TypeError('MessageFormatter: hooks must be synchronous');
        if (!this.#hooks.has(stage)) throw new RangeError(`MessageFormatter: unknown stage '${stage}'`);
        this.#hooks.get(stage).push({ fn, order });
    }

    /**
     * Extension failures are reported and isolated, as in upstream SillyTavern.
     * @param {string} stage
     * @param {string} text
     * @param {MessageFormattingBase} base
     * @returns {string}
     */
    runStage(stage, text, base) {
        const bucket = this.#hooks.get(stage);
        if (!bucket?.length) return text;
        const context = Object.freeze({ ...base, stage });
        for (const { fn } of bucket.slice().sort((a, b) => a.order - b.order)) {
            try {
                const result = fn(text, context);
                if (typeof result === 'string') {
                    text = result;
                } else {
                    console.warn(`[MessageFormatter] Hook at '${stage}' must return a string; its result was ignored.`);
                }
            } catch (error) {
                console.error(`[MessageFormatter] Hook error at '${stage}':`, error);
            }
        }
        return text;
    }

    /** @type {typeof messageFormatting} */
    format = (...args) => messageFormatting(...args);
}

/** The same singleton is available from getContext().messageFormatter. */
export const MessageFormatter = new MessageFormatterRegistry();
