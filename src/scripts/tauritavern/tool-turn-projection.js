import { IGNORE_SYMBOL } from '../constants.js';

/**
 * Projects the flat chat transcript into provider turns without mutating it.
 * Canonical Tool rows are matched by ID, so tool side effects may append other
 * chat messages between the Assistant call and its results. Invalid history
 * throws immediately and must not be sent to a provider.
 *
 * @param {ChatMessage[]} chat
 * @param {boolean} stripOldToolCalls
 * @returns {Array<
 *     | { type: 'message', sourceIndex: number, message: ChatMessage }
 *     | {
 *         type: 'tool-turn',
 *         sourceIndices: number[],
 *         assistantMessage: ChatMessage | null,
 *         metadataMessage: ChatMessage,
 *         invocations: import('../tool-calling.js').ToolInvocation[],
 *       }
 *   >}
 */
export function projectToolTurns(chat, stripOldToolCalls = false) {
    if (!Array.isArray(chat)) {
        throw new TypeError('Chat history must be an array');
    }

    const canonicalCallsById = new Map();
    const canonicalCallsByOwner = new Map();

    for (let ownerIndex = 0; ownerIndex < chat.length; ownerIndex++) {
        const message = chat[ownerIndex];
        if (message?.tool_calls !== undefined
            && (!Array.isArray(message.tool_calls) || message.tool_calls.length > 0)
            && !isAssistant(message)) {
            fail(`chat[${ownerIndex}].tool_calls is only valid on an Assistant message`);
        }
        if (!isAssistant(message) || message.extra?.[IGNORE_SYMBOL]) {
            continue;
        }

        const canonicalCalls = readCanonicalToolCalls(message.tool_calls, `chat[${ownerIndex}].tool_calls`);
        if (canonicalCalls === null) {
            continue;
        }
        const legacyInvocations = message.extra?.tool_invocations;
        if (legacyInvocations !== undefined && !(Array.isArray(legacyInvocations) && legacyInvocations.length === 0)) {
            fail(`chat[${ownerIndex}] cannot contain both tool_calls and extra.tool_invocations`);
        }
        const calls = canonicalCalls.map(call => ({ ...call, ownerIndex }));
        canonicalCallsByOwner.set(ownerIndex, calls);
        for (const call of calls) {
            const callsWithId = canonicalCallsById.get(call.id) ?? [];
            callsWithId.push(call);
            canonicalCallsById.set(call.id, callsWithId);
        }
    }

    const resultByCall = new Map();
    for (let sourceIndex = 0; sourceIndex < chat.length; sourceIndex++) {
        const message = chat[sourceIndex];
        if (message?.role !== 'tool' || message.extra?.[IGNORE_SYMBOL]) {
            continue;
        }

        const issue = validateCanonicalToolMessage(message, sourceIndex);
        if (issue) {
            fail(issue);
        }
        const legacyInvocations = message.extra?.tool_invocations;
        if (legacyInvocations !== undefined && !(Array.isArray(legacyInvocations) && legacyInvocations.length === 0)) {
            fail(`chat[${sourceIndex}] cannot contain both role "tool" and extra.tool_invocations`);
        }

        const precedingCalls = (canonicalCallsById.get(message.tool_call_id) ?? [])
            .filter(call => call.ownerIndex < sourceIndex);
        const pendingCalls = precedingCalls.filter(call => !resultByCall.has(call));
        if (pendingCalls.length > 1) {
            fail(`chat[${sourceIndex}].tool_call_id matches multiple unresolved Assistant tool calls`);
        }
        const call = pendingCalls[0];
        if (!call) {
            const reason = precedingCalls.length > 0
                ? `chat[${sourceIndex}].tool_call_id duplicates an earlier Tool result`
                : `chat[${sourceIndex}].tool_call_id does not match any preceding Assistant tool call`;
            fail(reason);
        }
        resultByCall.set(call, { sourceIndex, message });
    }

    for (const calls of canonicalCallsById.values()) {
        for (const call of calls) {
            if (!resultByCall.has(call)) {
                fail(`${call.path} has no matching Tool result`);
            }
        }
    }

    const canonicalTurnByOwner = new Map();
    const canonicalToolSourceIndices = new Set();

    for (const [ownerIndex, calls] of canonicalCallsByOwner) {
        const results = calls.map(call => resultByCall.get(call));

        const invocations = calls.map((call, index) => toInvocation(call, results[index].message));
        canonicalTurnByOwner.set(ownerIndex, {
            type: 'tool-turn',
            sourceIndices: [ownerIndex, ...results.map(result => result.sourceIndex)],
            assistantMessage: chat[ownerIndex],
            metadataMessage: chat[ownerIndex],
            invocations,
        });
    }

    for (const result of resultByCall.values()) {
        canonicalToolSourceIndices.add(result.sourceIndex);
    }

    const entries = [];
    for (let sourceIndex = 0; sourceIndex < chat.length; sourceIndex++) {
        const message = chat[sourceIndex];

        if (canonicalToolSourceIndices.has(sourceIndex)) {
            continue;
        }

        const canonicalTurn = canonicalTurnByOwner.get(sourceIndex);
        if (canonicalTurn) {
            entries.push(canonicalTurn);
            continue;
        }

        if (message?.extra?.[IGNORE_SYMBOL]) {
            entries.push({ type: 'message', sourceIndex, message });
            continue;
        }

        const legacy = readLegacyToolInvocations(message, sourceIndex);
        if (legacy.invocations === null) {
            if (legacy.issue) {
                fail(legacy.issue);
            }
            entries.push({ type: 'message', sourceIndex, message });
            continue;
        }

        const previousMessage = chat[sourceIndex - 1];
        const previousEntry = entries.at(-1);
        const ownerIndex = sourceIndex - 1;
        const hasAdjacentOwner = isAssistant(previousMessage)
            && previousMessage.tool_calls === undefined
            && !previousMessage.extra?.[IGNORE_SYMBOL]
            && previousEntry?.type === 'message'
            && previousEntry.sourceIndex === ownerIndex;

        if (hasAdjacentOwner) {
            entries.pop();
            const turn = {
                type: 'tool-turn',
                sourceIndices: [ownerIndex, sourceIndex],
                assistantMessage: previousMessage,
                metadataMessage: message,
                invocations: legacy.invocations,
            };
            entries.push(turn);
            continue;
        }

        entries.push({
            type: 'tool-turn',
            sourceIndices: [sourceIndex],
            assistantMessage: null,
            metadataMessage: message,
            invocations: legacy.invocations,
        });
    }

    if (!stripOldToolCalls) {
        return entries;
    }

    const latestUserIndex = chat.findLastIndex(message => message?.is_user === true);
    return entries.filter(entry => entry.type !== 'tool-turn'
        || entry.sourceIndices.some(sourceIndex => sourceIndex >= latestUserIndex));
}

/**
 * Reads the exact SillyTavern 1.18 synthetic system-floor format. This path is
 * read-only; new chats must use first-class Assistant and Tool messages.
 * @param {ChatMessage} message
 * @param {number} sourceIndex
 * @returns {{ invocations: import('../tool-calling.js').ToolInvocation[] | null, issue: string | null }}
 */
export function readLegacyToolInvocations(message, sourceIndex) {
    const invocations = message?.extra?.tool_invocations;
    if (invocations === undefined) {
        return { invocations: null, issue: null };
    }
    if (Array.isArray(invocations) && invocations.length === 0) {
        return { invocations: null, issue: null };
    }
    if (message?.is_system !== true || message?.is_user !== false || message?.role === 'tool') {
        return { invocations: null, issue: `chat[${sourceIndex}].extra.tool_invocations is only valid on a legacy system tool floor` };
    }
    if (!Array.isArray(invocations)) {
        return { invocations: null, issue: `chat[${sourceIndex}].extra.tool_invocations is not an array` };
    }

    const normalized = [];
    const ids = new Set();
    for (let invocationIndex = 0; invocationIndex < invocations.length; invocationIndex++) {
        const invocation = invocations[invocationIndex];
        const path = `chat[${sourceIndex}].extra.tool_invocations[${invocationIndex}]`;
        if (typeof invocation?.id !== 'string' || !invocation.id.trim()) {
            return { invocations: null, issue: `${path}.id is not a non-empty string` };
        }
        if (ids.has(invocation.id)) {
            return { invocations: null, issue: `${path}.id duplicates an earlier tool call id` };
        }
        ids.add(invocation.id);
        if (typeof invocation.name !== 'string' || !invocation.name.trim()) {
            return { invocations: null, issue: `${path}.name is not a non-empty string` };
        }

        const parameters = stringifyLegacyValue(invocation.parameters);
        if (parameters === null) {
            return { invocations: null, issue: `${path}.parameters is not serializable` };
        }
        const result = invocation.result === undefined ? '' : stringifyLegacyValue(invocation.result);
        if (result === null) {
            return { invocations: null, issue: `${path}.result is not serializable` };
        }
        normalized.push(parameters === invocation.parameters && result === invocation.result
            ? invocation
            : { ...invocation, parameters, result });
    }

    return { invocations: normalized, issue: null };
}

/** @param {unknown} value */
function stringifyLegacyValue(value) {
    if (typeof value === 'string') {
        return value;
    }
    try {
        const serialized = JSON.stringify(value);
        return typeof serialized === 'string' ? serialized : null;
    } catch {
        return null;
    }
}

/** @param {unknown} calls @param {string} path */
function readCanonicalToolCalls(calls, path) {
    if (calls === undefined || (Array.isArray(calls) && calls.length === 0)) {
        return null;
    }
    if (!Array.isArray(calls)) {
        fail(`${path} is not an array`);
    }

    const normalized = [];
    const ids = new Set();
    for (let index = 0; index < calls.length; index++) {
        const call = calls[index];
        const callPath = `${path}[${index}]`;
        if (typeof call?.id !== 'string' || !call.id.trim()) {
            fail(`${callPath}.id is not a non-empty string`);
        }
        if (ids.has(call.id)) {
            fail(`${callPath}.id duplicates an earlier tool call id`);
        }
        ids.add(call.id);
        if (typeof call.name !== 'string' || !call.name.trim()) {
            fail(`${callPath}.name is not a non-empty string`);
        }
        if (typeof call.parameters !== 'string') {
            fail(`${callPath}.parameters is not a string`);
        }
        normalized.push({
            id: call.id,
            name: call.name,
            parameters: call.parameters,
            ...(typeof call.displayName === 'string' && call.displayName.trim() ? { displayName: call.displayName } : {}),
            ...(typeof call.signature === 'string' || call.signature === null ? { signature: call.signature } : {}),
            path: callPath,
        });
    }
    return normalized;
}

/** @param {ChatMessage} message @param {number} sourceIndex */
function validateCanonicalToolMessage(message, sourceIndex) {
    if (message.tool_calls !== undefined
        && (!Array.isArray(message.tool_calls) || message.tool_calls.length > 0)) {
        return `chat[${sourceIndex}] role "tool" cannot also contain tool_calls`;
    }
    if (typeof message.tool_call_id !== 'string' || !message.tool_call_id.trim()) {
        return `chat[${sourceIndex}].tool_call_id is not a non-empty string`;
    }
    if (typeof message.mes !== 'string') {
        return `chat[${sourceIndex}].mes is not a string`;
    }
    return null;
}

/** @param {any} call @param {ChatMessage} toolMessage */
function toInvocation(call, toolMessage) {
    return {
        id: call.id,
        ...(call.displayName !== undefined ? { displayName: call.displayName } : {}),
        name: call.name,
        parameters: call.parameters,
        result: toolMessage.mes,
        ...(call.signature !== undefined ? { signature: call.signature } : {}),
        ...(typeof toolMessage.error === 'boolean' ? { error: toolMessage.error } : {}),
    };
}

/** @param {unknown} message */
function isAssistant(message) {
    return message !== null
        && typeof message === 'object'
        && message.role !== 'tool'
        && message.is_user !== true
        && message.is_system !== true;
}

/** @param {string} reason */
function fail(reason) {
    throw new Error(`Cannot reconstruct tool history: ${reason}`);
}
