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
// runtime shape of `swipes`. Pre-rollback-strategy messages (older runs that
// committed before this field existed) default to the deleteMessage path,
// matching the previous behavior.
//
// Contract:
// - `messageId` is the stringified chat array index the host returned to
//   `resolve_agent_chat_commit`. We never trust array order; we match by
//   `extra.tauritavern.agent.runId === runId` before touching anything so a
//   re-saved message from a later run is never wiped.
// - Targets are deduped and processed back-to-front so chat indices stay
//   valid as messages are spliced out. (Swipe deletes don't shift indices,
//   but mixed runs / future targets might, so ordering stays defensive.)
// - If the message has already been removed (e.g. the user deleted it
//   manually) or the swipe array is already gone, the target is silently
//   skipped — this is the expected no-op path.

const TARGETED_RUN_PATH = ['extra', 'tauritavern', 'agent', 'runId'];
const ROLLBACK_META_PATH = ['extra', 'tauritavern', 'agent', 'rollback'];

export async function rollbackAgentRunDriftMessages({ runId, targets, script }) {
    const normalizedRunId = String(runId || '').trim();
    if (!normalizedRunId || !Array.isArray(targets) || targets.length === 0) {
        return { attempted: 0, deleted: 0, swipesRemoved: 0, skipped: 0 };
    }

    const chat = Array.isArray(script?.chat) ? script.chat : null;
    const deleteMessage = typeof script?.deleteMessage === 'function' ? script.deleteMessage : null;
    const deleteSwipe = typeof script?.deleteSwipe === 'function' ? script.deleteSwipe : null;
    if (!chat || !deleteMessage) {
        return { attempted: targets.length, deleted: 0, swipesRemoved: 0, skipped: targets.length };
    }

    const uniqueIds = new Set();
    for (const target of targets) {
        const parsed = parseMessageId(target?.messageId);
        if (parsed !== null) {
            uniqueIds.add(parsed);
        }
    }

    const sortedIds = [...uniqueIds].sort((a, b) => b - a);
    let deleted = 0;
    let swipesRemoved = 0;
    let skipped = 0;
    for (const index of sortedIds) {
        if (index < 0 || index >= chat.length) {
            skipped += 1;
            continue;
        }
        if (readNested(chat[index], TARGETED_RUN_PATH) !== normalizedRunId) {
            skipped += 1;
            continue;
        }
        const outcome = await rollbackOneMessage({
            index,
            message: chat[index],
            deleteSwipe,
            deleteMessage,
        });
        if (outcome === 'swipe') {
            swipesRemoved += 1;
        } else if (outcome === 'message') {
            deleted += 1;
        } else {
            skipped += 1;
        }
    }

    return { attempted: targets.length, deleted, swipesRemoved, skipped };
}

async function rollbackOneMessage({ index, message, deleteSwipe, deleteMessage }) {
    const rollback = readNested(message, ROLLBACK_META_PATH);
    if (
        rollback?.strategy === 'deleteSwipe'
        && deleteSwipe
        && Number.isInteger(rollback.swipeId)
        && rollback.swipeId >= 0
        && Array.isArray(message?.swipes)
        && message.swipes.length > 1
        && rollback.swipeId < message.swipes.length
    ) {
        await deleteSwipe(rollback.swipeId, index);
        return 'swipe';
    }

    await deleteMessage(index);
    return 'message';
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
