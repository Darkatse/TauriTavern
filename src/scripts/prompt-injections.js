'use strict';

/**
 * @enum {number}
 */
export const INJECTION_POSITION = {
    RELATIVE: 0,
    ABSOLUTE: 1,
    ATTACH_EXISTING: 2,
};

const ATTACH_ROLES = new Set(['system', 'user', 'assistant']);
const ATTACH_SIDES = new Set(['start', 'end']);

const PROMPT_MANAGER_OVERRIDE_KEYS = [
    'injection_position',
    'injection_depth',
    'injection_order',
    'role',
    'attach_role',
    'attach_index',
    'attach_side',
];

/**
 * Treat missing and unknown historical values as Relative, matching old presets.
 * @param {object} prompt Prompt-like object.
 * @returns {number}
 */
export function getPromptInjectionPosition(prompt) {
    const position = Number(prompt?.injection_position ?? INJECTION_POSITION.RELATIVE);
    return Object.values(INJECTION_POSITION).includes(position) ? position : INJECTION_POSITION.RELATIVE;
}

/**
 * @param {object} prompt Prompt-like object.
 * @param {number} position Expected injection position.
 * @returns {boolean}
 */
export function isPromptInjectionPosition(prompt, position) {
    return getPromptInjectionPosition(prompt) === position;
}

/**
 * Apply Prompt Manager marker overrides to a generated prompt.
 * @param {object} prompt Target prompt-like object.
 * @param {object|null|undefined} collectionPrompt Prompt Manager prompt.
 * @returns {object}
 */
export function applyPromptManagerOverrides(prompt, collectionPrompt) {
    if (!collectionPrompt) {
        return prompt;
    }

    for (const key of PROMPT_MANAGER_OVERRIDE_KEYS) {
        if (collectionPrompt[key] !== undefined) {
            prompt[key] = collectionPrompt[key];
        }
    }

    return prompt;
}

/**
 * Partition prompt collection by injection position.
 * @param {{ collection?: object[] }} prompts PromptCollection-like object.
 * @returns {{ userRelativePromptIds: string[], absolutePrompts: object[], attachedPrompts: object[] }}
 */
export function getPromptInjectionGroups(prompts) {
    const collection = Array.isArray(prompts?.collection) ? prompts.collection : [];

    return {
        userRelativePromptIds: collection
            .filter(prompt => prompt?.system_prompt === false && isPromptInjectionPosition(prompt, INJECTION_POSITION.RELATIVE))
            .map(prompt => prompt.identifier),
        absolutePrompts: collection
            .filter(prompt => isPromptInjectionPosition(prompt, INJECTION_POSITION.ABSOLUTE)),
        attachedPrompts: collection
            .filter(prompt => isPromptInjectionPosition(prompt, INJECTION_POSITION.ATTACH_EXISTING)),
    };
}

/**
 * @param {{ get?: (identifier: string) => object|null|undefined }} prompts PromptCollection-like object.
 * @param {string} identifier Prompt identifier.
 * @returns {object|null}
 */
export function getRelativePromptById(prompts, identifier) {
    const prompt = prompts?.get?.(identifier) ?? null;
    return prompt && isPromptInjectionPosition(prompt, INJECTION_POSITION.RELATIVE) ? prompt : null;
}

/**
 * Apply attach-existing prompts directly onto raw chat history messages.
 * @param {object[]} attachedPrompts Prompts configured for ATTACH_EXISTING.
 * @param {object[]} messages Raw chat history messages in latest-first order.
 * @param {object} [options]
 * @param {(message: string) => void} [options.warn] Warning sink.
 * @returns {number} Number of prompts applied.
 */
export function applyAttachedPromptsToMessages(attachedPrompts, messages, { warn = console.warn } = {}) {
    if (!Array.isArray(attachedPrompts) || !attachedPrompts.length || !Array.isArray(messages) || !messages.length) {
        return 0;
    }

    let applied = 0;

    for (const attachPrompt of attachedPrompts) {
        const attachContent = String(attachPrompt?.content ?? '').trim();
        const attachRole = String(attachPrompt?.attach_role ?? '').trim();
        const attachSide = String(attachPrompt?.attach_side ?? 'end').trim();
        const rawIndex = attachPrompt?.attach_index ?? 1;

        if (!attachContent) {
            continue;
        }

        if (!ATTACH_ROLES.has(attachRole)) {
            warn?.(`[PromptManager] Attach prompt "${describePrompt(attachPrompt)}" has an invalid target role: ${attachRole || '(empty)'}.`);
            continue;
        }

        const requestedIndex = normalizeAttachIndex(rawIndex, attachPrompt, warn);
        if (requestedIndex === null) {
            continue;
        }

        const chronologicalRoleMatches = messages
            .filter(message => message && !message.injected && message.role === attachRole)
            .slice()
            .reverse();

        if (!chronologicalRoleMatches.length) {
            warn?.(`[PromptManager] Attach prompt "${describePrompt(attachPrompt)}" found no existing ${attachRole} messages.`);
            continue;
        }

        const targetIndex = clampAttachIndex(requestedIndex, chronologicalRoleMatches.length);
        const targetMessage = chronologicalRoleMatches[targetIndex];
        const currentContent = String(targetMessage.content ?? '');
        const side = ATTACH_SIDES.has(attachSide) ? attachSide : 'end';

        if (!ATTACH_SIDES.has(attachSide)) {
            warn?.(`[PromptManager] Attach prompt "${describePrompt(attachPrompt)}" has an invalid attach side: ${attachSide || '(empty)'}. Using end.`);
        }

        targetMessage.content = side === 'start'
            ? [attachContent, currentContent].filter(Boolean).join('\n\n')
            : [currentContent, attachContent].filter(Boolean).join('\n\n');
        applied += 1;
    }

    return applied;
}

function describePrompt(prompt) {
    return prompt?.identifier || prompt?.name || '(unnamed)';
}

function normalizeAttachIndex(rawIndex, prompt, warn) {
    const index = Number(rawIndex);
    if (!Number.isFinite(index) || !Number.isInteger(index)) {
        warn?.(`[PromptManager] Attach prompt "${describePrompt(prompt)}" has an invalid message index: ${String(rawIndex)}.`);
        return null;
    }

    return index === 0 ? 1 : index;
}

function clampAttachIndex(index, length) {
    if (index > 0) {
        return Math.min(index - 1, length - 1);
    }

    return Math.max(length + index, 0);
}
