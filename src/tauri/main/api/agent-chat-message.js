// @ts-check

const AUTO_COMMIT_TEXT_EXTENSIONS = new Set(['md', 'markdown', 'txt', 'text']);

export function prepareGeneratedReplyForSave(script, rawText, generationType) {
    // saveReply is a low-level chat writer. Legacy generation runs cleanup
    // before saveReply, so Agent commit must preserve that boundary here.
    const type = String(generationType || 'normal').trim() || 'normal';
    return script.cleanUpMessage({
        getMessage: rawText,
        isImpersonate: type === 'impersonate',
        isContinue: type === 'continue',
        displayIncompleteSentences: false,
    });
}

export function prepareGeneratedReplyForDisplay(script, rawText, generationType) {
    const type = String(generationType || 'normal').trim() || 'normal';
    return script.cleanUpMessage({
        getMessage: rawText,
        isImpersonate: type === 'impersonate',
        isContinue: type === 'continue',
        displayIncompleteSentences: true,
    });
}

export function normalizeCommitMode(value) {
    const mode = String(value || 'replace').trim();
    if (mode !== 'replace' && mode !== 'append') {
        throw new Error('agent.chat_commit_mode_invalid: mode must be replace or append');
    }
    return mode;
}

export function initialCommitSaveType(generationType, mode) {
    const type = String(generationType || 'normal').trim() || 'normal';
    if (mode === 'append' || type === 'append' || type === 'continue' || type === 'appendFinal') {
        return 'normal';
    }
    return type;
}

export function isAutoCommitTextPath(path) {
    const name = String(path || '').split('/').at(-1) || '';
    const dot = name.lastIndexOf('.');
    return dot > 0 && AUTO_COMMIT_TEXT_EXTENSIONS.has(name.slice(dot + 1).toLowerCase());
}

export function getActiveMessageId(chat) {
    if (!Array.isArray(chat) || chat.length === 0) {
        throw new Error('agent.chat_commit_message_missing: saveReply did not create a chat message');
    }
    return chat.length - 1;
}

export function captureMessageTarget(state, chat, messageId, lengthBefore) {
    const message = chat[messageId];
    const swipeId = readMessageSwipeId(message);
    if (!message || typeof message !== 'object' || swipeId == null) {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }

    state.messageId = messageId;
    state.messageRef = message;
    state.swipeId = swipeId;
    // saveReply for type='swipe' / 'regenerate' against an existing
    // assistant message appends a new swipe in-place instead of pushing a new
    // chat entry. Rollback needs to distinguish those two cases.
    state.createdMessage = chat.length > lengthBefore;
    if (!state.createdMessage) {
        restoreAgentExtra(message, null);
    }
}

export function assertActiveAgentMessage(chat, state) {
    const messageId = Number(state.messageId);
    if (!Array.isArray(chat) || chat.length - 1 !== messageId) {
        throw new Error('agent.chat_commit_message_mismatch: this run can only update its active chat message');
    }
    const message = chat[messageId];
    if (!message
        || message !== state.messageRef
        || readMessageSwipeId(message) !== state.swipeId) {
        throw new Error('agent.chat_commit_message_mismatch: active chat message changed during this run');
    }
}

export async function emitGeneratedMessageEvents(script, messageId, generationType) {
    if (typeof script.eventSource?.emit !== 'function' || !script.event_types) {
        throw new Error('agent.message_events_unavailable: SillyTavern message events are unavailable');
    }
    const type = String(generationType || 'normal').trim() || 'normal';
    await script.eventSource.emit(script.event_types.MESSAGE_RECEIVED, messageId, type);
    await script.eventSource.emit(script.event_types.CHARACTER_MESSAGE_RENDERED, messageId, type);
}

export function mergeAgentCommitExtraIntoMessage(chat, messageId, payload, file, commitSeq, runState = {}) {
    if (!Array.isArray(chat) || chat.length <= messageId) {
        throw new Error('agent.chat_commit_message_missing: active chat message is missing');
    }

    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }

    const previousAgent = message.extra?.tauritavern?.agent;
    const previousCommits = Array.isArray(previousAgent?.commits) ? previousAgent.commits : [];
    const chars = requireNonNegativeInteger(file?.chars, 'chars');
    const words = requireNonNegativeInteger(file?.words, 'words');
    const commit = {
        seq: commitSeq,
        commitId: payload.commitId,
        path: file.path,
        mode: normalizeCommitMode(payload.mode),
        reason: typeof payload.reason === 'string' ? payload.reason : undefined,
        chars,
        words,
        sha256: file.sha256,
    };
    const createdMessage = runState.createdMessage !== false;
    const swipeId = Number(runState.swipeId);
    const rollback = createdMessage || !Number.isInteger(swipeId) || swipeId < 0
        ? { strategy: 'deleteMessage' }
        : { strategy: 'deleteSwipe', swipeId };
    mergeAgentExtra(message, {
        version: 2,
        runId: payload.runId,
        workspaceId: payload.workspaceId,
        stableChatId: payload.stableChatId,
        profileId: payload.profileId ?? null,
        persistBaseStateId: payload.persistBaseStateId ?? null,
        persistStateStatus: 'not_committed',
        commitId: payload.commitId,
        commitSeq,
        commits: [...previousCommits, commit],
        rollback,
        artifacts: [{
            path: file.path,
            target: 'message_body',
            chars,
            words,
            sha256: file.sha256,
        }],
    });
}

export function mergePersistentStateExtraIntoMessage(chat, messageId, payload, stateId) {
    if (!Array.isArray(chat) || chat.length <= messageId) {
        throw new Error('agent.persistent_state_message_missing: target chat message is missing');
    }

    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.persistent_state_message_invalid: target chat message is invalid');
    }
    if (message.extra?.tauritavern?.agent?.runId !== payload.runId) {
        throw new Error('agent.persistent_state_message_mismatch: target message belongs to another run');
    }

    mergeAgentExtra(message, {
        persistStateId: stateId,
        persistBaseStateId: payload.baseStateId ?? null,
        persistStateStatus: 'committed',
        persistChangeCount: Number(payload.changeCount ?? 0),
    });
}

export function snapshotAgentExtra(message) {
    return structuredClone(message?.extra?.tauritavern?.agent ?? null);
}

export function restoreAgentExtra(message, snapshot) {
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }
    if (snapshot === null) {
        const tauritavern = message.extra?.tauritavern;
        if (tauritavern && typeof tauritavern === 'object') {
            delete tauritavern.agent;
            if (Object.keys(tauritavern).length === 0) delete message.extra.tauritavern;
        }
    } else {
        message.extra ??= {};
        message.extra.tauritavern = {
            ...(message.extra.tauritavern || {}),
            agent: structuredClone(snapshot),
        };
    }
    syncActiveSwipeExtra(message);
}

export async function persistActiveChat(script, commitReason) {
    const groupChats = await import('../../../scripts/group-chats.js');
    if (groupChats.selected_group) {
        if (typeof groupChats.saveGroupChat !== 'function') {
            throw new Error('saveGroupChat is not available');
        }
        await groupChats.saveGroupChat(groupChats.selected_group, true, false, commitReason);
        return;
    }

    if (typeof script.saveChat !== 'function') {
        throw new Error('saveChat is not available');
    }
    await script.saveChat({ commitReason });
}

function readMessageSwipeId(message) {
    const swipeId = Number(message?.swipe_id);
    return Number.isInteger(swipeId) && swipeId >= 0 ? swipeId : null;
}

function syncActiveSwipeExtra(message) {
    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }
}

function mergeAgentExtra(message, patch) {
    message.extra ??= {};
    message.extra.tauritavern = {
        ...message.extra.tauritavern,
        agent: {
            ...message.extra.tauritavern?.agent,
            ...patch,
        },
    };
    syncActiveSwipeExtra(message);
}

function requireNonNegativeInteger(value, key) {
    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`agent.host_workspace_file_invalid: ${key} must be a non-negative integer`);
    }
    return number;
}
