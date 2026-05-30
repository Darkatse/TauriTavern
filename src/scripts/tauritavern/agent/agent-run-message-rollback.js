// Rolls back chat artifacts an Agent run committed when the user or a legacy
// run flow explicitly discards partial output. `run_rollback_targets` carries
// the host-assigned messageId (chat array index returned by
// agent-chat-commit-bridge.js); per target we either:
//
// - pop just the swipe this run added to a pre-existing assistant message
//   (when the run's first commit was a regenerate / swipe), so prior swipes
//   the user authored are preserved; or
// - delete the whole chat entry (when the run's first commit created a brand
//   new message).
//
// The strategy is recorded at commit time on
// `extra.tauritavern.agent.rollback` so we never have to guess from the
// runtime shape of `swipes`.
//
// Contract:
// - `messageId` is the stringified chat array index the host returned to
//   `resolve_agent_chat_commit`. We verify
//   `extra.tauritavern.agent.runId === runId` before touching anything so a
//   re-saved message from a later run is surfaced as a contract violation.
// - Targets are deduped and processed back-to-front so chat indices stay
//   valid as messages are spliced out.
// - After a successful destructive rollback, surviving messages revealed by
//   the rollback receive MESSAGE_UPDATED so DOM-driven render adapters can
//   rescan their current HTML/runtime state.
// - Destructive rollback is fail-fast. Missing host APIs, invalid targets, run
//   mismatches, missing strategy metadata, and unsafe swipe state throw instead
//   of silently falling back to a broader delete.

const TARGETED_RUN_PATH = ['extra', 'tauritavern', 'agent', 'runId'];
const ROLLBACK_META_PATH = ['extra', 'tauritavern', 'agent', 'rollback'];

export async function rollbackAgentRunDriftMessages({ runId, targets, script }) {
    const normalizedRunId = requireRollbackRunId(runId);
    const sortedIds = normalizeRollbackTargets(targets);
    if (sortedIds.length === 0) {
        return { attempted: 0, deleted: 0, swipesRemoved: 0 };
    }

    const chat = requireChatArray(script);
    const emitMessageUpdated = requireMessageUpdatedEmitter(script);
    const messagesToUpdate = new Set();
    let deleted = 0;
    let swipesRemoved = 0;
    for (const index of sortedIds) {
        if (index < 0 || index >= chat.length) {
            throw new Error(`agent.rollback_target_missing: chat message ${index} is not available`);
        }
        if (readNested(chat[index], TARGETED_RUN_PATH) !== normalizedRunId) {
            throw new Error(`agent.rollback_run_mismatch: chat message ${index} does not belong to run ${normalizedRunId}`);
        }
        const outcome = await rollbackOneMessage({
            index,
            message: chat[index],
            script,
            chat,
        });
        rememberMessageForUpdate(messagesToUpdate, outcome.refreshMessage);
        if (outcome.type === 'swipe') {
            swipesRemoved += 1;
        } else if (outcome.type === 'message') {
            deleted += 1;
        }
    }
    await emitRollbackMessageUpdates(chat, messagesToUpdate, emitMessageUpdated);

    return { attempted: sortedIds.length, deleted, swipesRemoved };
}

async function rollbackOneMessage({ index, message, script, chat }) {
    const rollback = readNested(message, ROLLBACK_META_PATH);
    const strategy = String(rollback?.strategy || '').trim();
    if (!strategy) {
        throw new Error(`agent.rollback_strategy_missing: chat message ${index} has no rollback strategy`);
    }

    if (strategy === 'deleteMessage') {
        const deleteMessage = requireHostFunction(script, 'deleteMessage');
        await deleteMessage(index);
        if (chat.includes(message)) {
            throw new Error(`agent.rollback_message_delete_failed: chat message ${index} was not removed`);
        }
        return { type: 'message', refreshMessage: messageRevealedByDelete(chat, index) };
    }

    if (strategy === 'deleteSwipe') {
        const deleteSwipe = requireHostFunction(script, 'deleteSwipe');
        const swipeId = requireRollbackSwipeId(rollback?.swipeId, index);
        assertSafeSwipeRollbackTarget(message, swipeId, index);
        const nextSwipeId = await deleteSwipe(swipeId, index);
        if (!Number.isInteger(Number(nextSwipeId))) {
            throw new Error(`agent.rollback_swipe_delete_failed: deleteSwipe did not confirm deletion for message ${index}`);
        }
        return { type: 'swipe', refreshMessage: message };
    }

    throw new Error(`agent.rollback_strategy_unsupported: unsupported rollback strategy ${strategy}`);
}

function requireRollbackRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('agent.rollback_run_id_required: runId is required');
    }
    return runId;
}

function normalizeRollbackTargets(targets) {
    if (!Array.isArray(targets)) {
        throw new Error('agent.rollback_targets_invalid: targets must be an array');
    }

    const uniqueIds = new Set();
    targets.forEach((target, position) => {
        const parsed = parseMessageId(target?.messageId);
        if (parsed === null) {
            throw new Error(`agent.rollback_target_invalid: targets[${position}].messageId must be a non-negative integer`);
        }
        uniqueIds.add(parsed);
    });

    return [...uniqueIds].sort((a, b) => b - a);
}

function requireChatArray(script) {
    if (!Array.isArray(script?.chat)) {
        throw new Error('agent.rollback_chat_unavailable: SillyTavern chat array is unavailable');
    }
    return script.chat;
}

function requireHostFunction(script, name) {
    const value = script?.[name];
    if (typeof value !== 'function') {
        throw new Error(`agent.rollback_host_api_unavailable: ${name} is unavailable`);
    }
    return value;
}

function requireMessageUpdatedEmitter(script) {
    const emit = script?.eventSource?.emit;
    if (typeof emit !== 'function') {
        throw new Error('agent.rollback_event_api_unavailable: eventSource.emit is unavailable');
    }
    const eventType = String(script?.event_types?.MESSAGE_UPDATED || '').trim();
    if (!eventType) {
        throw new Error('agent.rollback_event_type_unavailable: event_types.MESSAGE_UPDATED is unavailable');
    }

    return async (messageId) => {
        await emit.call(script.eventSource, eventType, messageId);
    };
}

function requireRollbackSwipeId(value, messageId) {
    if (!Number.isInteger(value) || value < 0) {
        throw new Error(`agent.rollback_swipe_id_invalid: rollback swipeId is invalid for message ${messageId}`);
    }
    return value;
}

function assertSafeSwipeRollbackTarget(message, swipeId, messageId) {
    if (!Array.isArray(message?.swipes)) {
        throw new Error(`agent.rollback_swipe_state_invalid: message ${messageId} has no swipe array`);
    }
    if (message.swipes.length <= 1) {
        throw new Error(`agent.rollback_swipe_state_invalid: message ${messageId} has no prior swipe to preserve`);
    }
    if (swipeId >= message.swipes.length) {
        throw new Error(`agent.rollback_swipe_id_invalid: swipe ${swipeId} is out of range for message ${messageId}`);
    }
}

function messageRevealedByDelete(chat, deletedIndex) {
    return chat[deletedIndex] ?? chat[deletedIndex - 1] ?? null;
}

function rememberMessageForUpdate(messagesToUpdate, message) {
    if (message && typeof message === 'object') {
        messagesToUpdate.add(message);
    }
}

async function emitRollbackMessageUpdates(chat, messagesToUpdate, emitMessageUpdated) {
    const messageIds = new Set();
    for (const message of messagesToUpdate) {
        const index = chat.indexOf(message);
        if (index >= 0) {
            messageIds.add(index);
        }
    }

    for (const messageId of [...messageIds].sort((a, b) => a - b)) {
        await emitMessageUpdated(messageId);
    }
}

function parseMessageId(value) {
    if (value == null) {
        return null;
    }
    const text = String(value).trim();
    if (!text) {
        return null;
    }
    const parsed = Number(text);
    if (!Number.isInteger(parsed) || parsed < 0) {
        return null;
    }
    return parsed;
}

function readNested(source, path) {
    let cursor = source;
    for (const key of path) {
        if (!cursor || typeof cursor !== 'object') {
            return undefined;
        }
        cursor = cursor[key];
    }
    return cursor;
}
