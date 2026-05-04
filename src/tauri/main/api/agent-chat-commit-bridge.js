// @ts-check

const activeCommitBridges = new Map();

export function attachHostCommitBridge({ runId, safeInvoke, readWorkspaceFile, subscribe }) {
    const normalizedRunId = requireRunId(runId);
    if (activeCommitBridges.has(normalizedRunId)) {
        return activeCommitBridges.get(normalizedRunId);
    }

    const state = {
        runId: normalizedRunId,
        messageId: null,
        commitSeq: 0,
        resolvedCommitIds: new Set(),
        stop: null,
    };
    const stop = subscribe(normalizedRunId, (event) => {
        if (event?.type === 'chat_commit_requested') {
            void handleChatCommitRequested({
                state,
                event,
                safeInvoke,
                readWorkspaceFile,
            }).catch((error) => {
                queueMicrotask(() => {
                    throw error;
                });
            });
            return;
        }

        if (event?.type === 'run_completed' || event?.type === 'run_failed' || event?.type === 'run_cancelled') {
            detachHostCommitBridge(normalizedRunId);
        }
    }, {
        onError(error) {
            queueMicrotask(() => {
                throw error;
            });
        },
    });

    state.stop = stop;
    activeCommitBridges.set(normalizedRunId, state);
    return state;
}

function detachHostCommitBridge(runId) {
    const normalizedRunId = requireRunId(runId);
    const state = activeCommitBridges.get(normalizedRunId);
    if (!state) {
        return;
    }
    activeCommitBridges.delete(normalizedRunId);
    if (typeof state.stop === 'function') {
        state.stop();
    }
}

async function handleChatCommitRequested({ state, event, safeInvoke, readWorkspaceFile }) {
    const payload = event?.payload || {};
    const commitId = requireCommitPayloadString(payload, 'commitId');
    if (state.resolvedCommitIds.has(commitId)) {
        return;
    }
    state.resolvedCommitIds.add(commitId);

    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        const path = requireCommitPayloadString(payload, 'path');
        const mode = normalizeCommitMode(payload.mode);
        const file = await readWorkspaceFile({ runId: state.runId, path });
        const script = await import('../../../script.js');
        if (typeof script.saveReply !== 'function') {
            throw new Error('saveReply is not available');
        }

        const isFirstCommit = state.messageId == null;
        let messageId;
        if (isFirstCommit) {
            await script.saveReply({
                type: initialCommitSaveType(payload.generationType, mode),
                getMessage: String(file?.text ?? ''),
            });
            messageId = getActiveMessageId(script.chat);
            state.messageId = messageId;
        } else {
            messageId = Number(state.messageId);
            assertActiveAgentMessage(script.chat, messageId, state.runId);
            await script.saveReply({
                type: mode === 'append' ? 'append' : 'appendFinal',
                getMessage: String(file?.text ?? ''),
            });
        }

        state.commitSeq += 1;
        mergeAgentCommitExtraIntoMessage(script.chat, messageId, payload, file, state.commitSeq);
        await persistActiveChat(script);

        await safeInvoke('resolve_agent_chat_commit', {
            dto: {
                runId: state.runId,
                commitId,
                messageId: String(messageId),
            },
        });
    } catch (error) {
        await safeInvoke('resolve_agent_chat_commit', {
            dto: {
                runId: state.runId,
                commitId,
                error: String(error?.message ?? error),
            },
        });
    }
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function requireCommitPayloadString(payload, key) {
    const value = String(payload?.[key] || '').trim();
    if (!value) {
        throw new Error(`agent.chat_commit_payload_invalid: ${key} is required`);
    }
    return value;
}

function normalizeCommitMode(value) {
    const mode = String(value || 'replace').trim();
    if (mode !== 'replace' && mode !== 'append') {
        throw new Error('agent.chat_commit_mode_invalid: mode must be replace or append');
    }
    return mode;
}

function initialCommitSaveType(generationType, mode) {
    const type = String(generationType || 'normal').trim() || 'normal';
    if (mode === 'append' || type === 'append' || type === 'continue' || type === 'appendFinal') {
        return 'normal';
    }
    return type;
}

function getActiveMessageId(chat) {
    if (!Array.isArray(chat) || chat.length === 0) {
        throw new Error('agent.chat_commit_message_missing: saveReply did not create a chat message');
    }
    return chat.length - 1;
}

function assertActiveAgentMessage(chat, messageId, runId) {
    if (!Array.isArray(chat) || chat.length - 1 !== messageId) {
        throw new Error('agent.chat_commit_message_mismatch: this run can only update its active chat message');
    }
    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }
    const messageRunId = message.extra?.tauritavern?.agent?.runId;
    if (messageRunId !== runId) {
        throw new Error('agent.chat_commit_message_mismatch: active chat message belongs to another run');
    }
}

function mergeAgentCommitExtraIntoMessage(chat, messageId, payload, file, commitSeq) {
    if (!Array.isArray(chat) || chat.length <= messageId) {
        throw new Error('agent.chat_commit_message_missing: active chat message is missing');
    }

    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }

    const previousAgent = message.extra?.tauritavern?.agent;
    const previousCommits = Array.isArray(previousAgent?.commits) ? previousAgent.commits : [];
    const commit = {
        seq: commitSeq,
        commitId: payload.commitId,
        checkpointId: payload.checkpointId,
        path: file.path,
        mode: normalizeCommitMode(payload.mode),
        reason: typeof payload.reason === 'string' ? payload.reason : undefined,
        bytes: file.bytes,
        sha256: file.sha256,
    };
    const extra = {
        tauritavern: {
            agent: {
                version: 1,
                runId: payload.runId,
                workspaceId: payload.workspaceId,
                stableChatId: payload.stableChatId,
                profileId: payload.profileId ?? null,
                checkpointId: payload.checkpointId,
                commitId: payload.commitId,
                commitSeq,
                commits: [...previousCommits, commit],
                artifacts: [{
                    path: file.path,
                    target: 'message_body',
                    bytes: file.bytes,
                    sha256: file.sha256,
                }],
            },
        },
    };

    message.extra = mergePlainObject(message.extra, extra);
    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }
}

async function assertCurrentChat(expectedRef, expectedStableChatId = null) {
    const currentRef = window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
    if (!sameChatRef(currentRef, expectedRef)) {
        const expectedStable = String(expectedStableChatId || '').trim();
        if (expectedStable) {
            const currentStable = await resolveStableChatId(currentRef);
            if (currentStable === expectedStable) {
                return;
            }
        }

        throw new Error('agent.commit_chat_mismatch: active chat changed before commit');
    }
}

function sameChatRef(a, b) {
    if (!a || !b || a.kind !== b.kind) {
        return false;
    }
    if (a.kind === 'character') {
        return String(a.characterId || '') === String(b.characterId || '')
            && String(a.fileName || '') === String(b.fileName || '');
    }
    return String(a.chatId || '') === String(b.chatId || '');
}

async function resolveStableChatId(chatRef) {
    const chatApi = window.__TAURITAVERN__?.api?.chat;
    if (!chatApi || typeof chatApi.open !== 'function') {
        throw new Error('api.chat is required to resolve stableChatId');
    }

    const handle = chatApi.open(chatRef);
    if (!handle || typeof handle.stableId !== 'function') {
        throw new Error('api.chat.open(ref).stableId is required to resolve stableChatId');
    }

    return String(await handle.stableId()).trim();
}

function mergePlainObject(base, patch) {
    const output = isPlainObject(base) ? { ...base } : {};
    if (!isPlainObject(patch)) {
        return output;
    }

    for (const [key, value] of Object.entries(patch)) {
        if (isPlainObject(value) && isPlainObject(output[key])) {
            output[key] = mergePlainObject(output[key], value);
        } else {
            output[key] = value;
        }
    }

    return output;
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

async function persistActiveChat(script) {
    const groupChats = await import('../../../scripts/group-chats.js');
    if (groupChats.selected_group) {
        if (typeof groupChats.saveGroupChat !== 'function') {
            throw new Error('saveGroupChat is not available');
        }
        await groupChats.saveGroupChat(groupChats.selected_group, true);
        return;
    }

    if (typeof script.saveChat !== 'function') {
        throw new Error('saveChat is not available');
    }
    await script.saveChat();
}
