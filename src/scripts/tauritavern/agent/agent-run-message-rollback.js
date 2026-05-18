// Rolls back chat messages that an Agent run committed before it failed
// because of instruction drift. The backend emits `run_rollback_targets`
// with the host-assigned messageId (chat array index, see
// src/tauri/main/api/agent-chat-commit-bridge.js); we delete the message
// in place using the vendor-exported `deleteMessage` so all the usual
// bookkeeping (DOM removal, persistent state pruning, save) runs.
//
// Contract:
// - `messageId` is the stringified chat array index the host returned to
//   `resolve_agent_chat_commit`. We never trust array order; we match by
//   `extra.tauritavern.agent.runId === runId` before deleting so a
//   re-saved message from a later run is never wiped.
// - Targets are deduped and processed back-to-front so chat indices stay
//   valid as messages are spliced out.
// - If the message has already been removed (e.g. the user deleted it
//   manually), the target is silently skipped — this is the expected
//   no-op path.

const TARGETED_RUN_PATH = ['extra', 'tauritavern', 'agent', 'runId'];

export async function rollbackAgentRunDriftMessages({ runId, targets, script }) {
    const normalizedRunId = String(runId || '').trim();
    if (!normalizedRunId || !Array.isArray(targets) || targets.length === 0) {
        return { attempted: 0, deleted: 0, skipped: 0 };
    }

    const chat = Array.isArray(script?.chat) ? script.chat : null;
    const deleteMessage = typeof script?.deleteMessage === 'function' ? script.deleteMessage : null;
    if (!chat || !deleteMessage) {
        return { attempted: targets.length, deleted: 0, skipped: targets.length };
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
        try {
            await deleteMessage(index);
            deleted += 1;
        } catch (error) {
            console.error('[TauriTavern] Failed to rollback Agent drift chat message', { index, runId: normalizedRunId, error });
            skipped += 1;
        }
    }

    return { attempted: targets.length, deleted, skipped };
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
