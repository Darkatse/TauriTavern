// @ts-check

import { resolveStableChatId } from './agent-chat-identity.js';

const activeCommitBridges = new Map();
const TERMINAL_EVENTS = new Set(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);

export function attachHostCommitBridge({ runId, safeInvoke, readWorkspaceFile, subscribe, loadScript = loadMainScript, persistChat = persistActiveChat }) {
    const normalizedRunId = requireRunId(runId);
    if (activeCommitBridges.has(normalizedRunId)) {
        return activeCommitBridges.get(normalizedRunId);
    }

    const state = {
        runId: normalizedRunId,
        messageId: null,
        // `createdMessage` and `firstSwipeId` are captured on the first commit so
        // a later `run_rollback_targets` can tell whether deleting the run means
        // removing the whole chat entry or only popping the swipe this run added
        // to a pre-existing assistant message (regenerate / swipe generation
        // types). See agent-run-message-rollback.js for the consumer.
        createdMessage: null,
        firstSwipeId: null,
        // Keep the raw chat target for this run. `append` adds the selected
        // file text as the next raw contribution, then cleanup runs on the
        // whole target so storage-time regex can match across commit chunks.
        rawCommittedText: '',
        commitSeq: 0,
        resolvedCommitIds: new Set(),
        resolvedPersistentStateUpdateIds: new Set(),
        loadScript,
        persistChat,
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

        if (event?.type === 'persistent_state_metadata_update_requested') {
            void handlePersistentStateMetadataUpdateRequested({
                state,
                event,
                safeInvoke,
            }).catch((error) => {
                queueMicrotask(() => {
                    throw error;
                });
            });
            return;
        }

        if (TERMINAL_EVENTS.has(event?.type)) {
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
    const commitId = requirePayloadString(payload, 'commitId');
    if (state.resolvedCommitIds.has(commitId)) {
        return;
    }
    state.resolvedCommitIds.add(commitId);

    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        const path = requirePayloadString(payload, 'path');
        const mode = normalizeCommitMode(payload.mode);
        const file = await readWorkspaceFile({ runId: state.runId, path });
        const script = await state.loadScript();
        if (typeof script.saveReply !== 'function') {
            throw new Error('saveReply is not available');
        }
        if (typeof script.cleanUpMessage !== 'function') {
            throw new Error('cleanUpMessage is not available');
        }

        const rawCommitText = String(file?.text ?? '');
        const rawCommittedText = mode === 'append'
            ? state.rawCommittedText + rawCommitText
            : rawCommitText;
        const getMessage = prepareGeneratedReplyForSave(script, rawCommittedText, payload.generationType);

        const isFirstCommit = state.messageId == null;
        let messageId;
        if (isFirstCommit) {
            const lengthBefore = script.chat.length;
            await script.saveReply({
                type: initialCommitSaveType(payload.generationType, mode),
                getMessage,
            });
            messageId = getActiveMessageId(script.chat);
            state.messageId = messageId;
            // saveReply for type='swipe' / 'regenerate' against an existing
            // assistant message appends a new swipe in-place instead of pushing
            // a new chat entry. We snapshot which case we're in so rollback can
            // pop just the run's swipe (preserving prior swipes) rather than
            // wiping the entire message.
            state.createdMessage = script.chat.length > lengthBefore;
            state.firstSwipeId = state.createdMessage ? null : readMessageSwipeId(script.chat[messageId]);
        } else {
            messageId = Number(state.messageId);
            assertActiveAgentMessage(script.chat, messageId, state.runId);
            // `getMessage` is the complete cleaned target, not an append
            // fragment. Use appendFinal to rewrite this run's chat message.
            await script.saveReply({
                type: 'appendFinal',
                getMessage,
            });
        }

        state.rawCommittedText = rawCommittedText;
        state.commitSeq += 1;
        mergeAgentCommitExtraIntoMessage(script.chat, messageId, payload, file, state.commitSeq, {
            createdMessage: state.createdMessage,
            firstSwipeId: state.firstSwipeId,
        });
        await state.persistChat(script);

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

async function handlePersistentStateMetadataUpdateRequested({ state, event, safeInvoke }) {
    const payload = event?.payload || {};
    const updateId = requirePayloadString(payload, 'updateId');
    if (state.resolvedPersistentStateUpdateIds.has(updateId)) {
        return;
    }
    state.resolvedPersistentStateUpdateIds.add(updateId);

    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        const script = await state.loadScript();
        const messageId = normalizeMessageId(payload.messageId ?? state.messageId);
        const stateId = requirePayloadString(payload, 'stateId');
        mergePersistentStateExtraIntoMessage(script.chat, messageId, payload, stateId);
        await state.persistChat(script);

        await safeInvoke('resolve_agent_persistent_state_metadata_update', {
            dto: {
                runId: state.runId,
                updateId,
            },
        });
    } catch (error) {
        await safeInvoke('resolve_agent_persistent_state_metadata_update', {
            dto: {
                runId: state.runId,
                updateId,
                error: String(error?.message ?? error),
            },
        });
    }
}

async function loadMainScript() {
    return import('../../../script.js');
}

function prepareGeneratedReplyForSave(script, rawText, generationType) {
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

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function normalizeMessageId(value) {
    const messageId = Number(value);
    if (!Number.isInteger(messageId) || messageId < 0) {
        throw new Error('agent.persistent_state_message_id_invalid: messageId must be a non-negative integer');
    }
    return messageId;
}

function requirePayloadString(payload, key) {
    const value = String(payload?.[key] || '').trim();
    if (!value) {
        throw new Error(`agent.host_payload_invalid: ${key} is required`);
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

function readMessageSwipeId(message) {
    const swipeId = Number(message?.swipe_id);
    return Number.isInteger(swipeId) && swipeId >= 0 ? swipeId : null;
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

function mergeAgentCommitExtraIntoMessage(chat, messageId, payload, file, commitSeq, runState = {}) {
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
        checkpointId: payload.checkpointId,
        path: file.path,
        mode: normalizeCommitMode(payload.mode),
        reason: typeof payload.reason === 'string' ? payload.reason : undefined,
        chars,
        words,
        sha256: file.sha256,
    };
    const rollback = (() => {
        // Default to deleting the whole message so messages persisted before
        // this field was introduced keep the old rollback behavior.
        const createdMessage = runState.createdMessage !== false;
        const swipeId = Number(runState.firstSwipeId);
        if (createdMessage || !Number.isInteger(swipeId) || swipeId < 0) {
            return { strategy: 'deleteMessage' };
        }
        return { strategy: 'deleteSwipe', swipeId };
    })();
    const extra = {
        tauritavern: {
            agent: {
                version: 2,
                runId: payload.runId,
                workspaceId: payload.workspaceId,
                stableChatId: payload.stableChatId,
                profileId: payload.profileId ?? null,
                persistBaseStateId: payload.persistBaseStateId ?? null,
                persistStateStatus: 'not_committed',
                checkpointId: payload.checkpointId,
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
            },
        },
    };

    message.extra = mergePlainObject(message.extra, extra);
    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }
}

function mergePersistentStateExtraIntoMessage(chat, messageId, payload, stateId) {
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

    const extra = {
        tauritavern: {
            agent: {
                persistStateId: stateId,
                persistBaseStateId: payload.baseStateId ?? null,
                persistStateStatus: 'committed',
                persistChangeCount: Number(payload.changeCount ?? 0),
            },
        },
    };

    message.extra = mergePlainObject(message.extra, extra);
    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }
}

function requireNonNegativeInteger(value, key) {
    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`agent.host_workspace_file_invalid: ${key} must be a non-negative integer`);
    }
    return number;
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
