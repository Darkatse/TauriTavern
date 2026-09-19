// @ts-check

export function createCommitReasoningState() {
    return {
        commitInvocationIds: new Set(['inv_root']),
        turns: [],
        cursor: 0,
    };
}

export function trackCommitReasoningEvent(state, event) {
    const payload = event?.payload || {};
    if (event?.type === 'agent_invocation_created' && payload.exitPolicy === 'run_finish_allowed') {
        state.commitInvocationIds.add(String(payload.invocationId));
    } else if (event?.type === 'model_completed'
        && payload.hasReasoning === true
        && state.commitInvocationIds.has(String(payload.invocationId || 'inv_root'))) {
        state.turns.push({
            invocationId: String(payload.invocationId || 'inv_root'),
            round: Number(payload.round),
            maxChars: Number(payload.reasoningChars),
        });
    }
}

export async function prepareCommitReasoning(state, runId, readModelTurn) {
    const pending = state.turns.slice(state.cursor);
    if (pending.length === 0) return { cursor: state.cursor, delta: '' };
    if (typeof readModelTurn !== 'function') {
        throw new Error('agent.model_turn_reader_unavailable: readModelTurn is not available');
    }

    const text = [];
    for (const turnRef of pending) {
        const parts = reasoningParts(await readModelTurn({ runId, ...turnRef }));
        if (parts.some(part => part.truncated)) {
            throw new Error('agent.model_turn_reasoning_truncated: reasoning is incomplete');
        }
        text.push(...parts.map(part => String(part.text || '').trim()).filter(Boolean));
    }

    const delta = text.join('\n\n');
    if (!delta) {
        throw new Error('agent.model_turn_reasoning_missing: reasoning summary has no visible text');
    }
    return {
        cursor: state.turns.length,
        delta: state.cursor > 0 ? `\n\n${delta}` : delta,
    };
}

export function commitPreparedReasoning(state, prepared) {
    state.cursor = prepared.cursor;
}

function reasoningParts(turn) {
    if (!Array.isArray(turn?.reasoning)) {
        throw new Error('agent.model_turn_reasoning_invalid: reasoning must be an array');
    }
    return turn.reasoning;
}
