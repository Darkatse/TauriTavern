// @ts-check

import { assertCurrentChat } from './agent-chat-identity.js';
import {
    assertActiveAgentMessage,
    captureMessageTarget,
    emitGeneratedMessageEvents,
    getActiveMessageId,
    initialCommitSaveType,
    isAutoCommitTextPath,
    mergeAgentCommitExtraIntoMessage,
    mergePersistentStateExtraIntoMessage,
    normalizeCommitMode,
    persistActiveChat,
    prepareGeneratedReplyForDisplay,
    prepareGeneratedReplyForSave,
    restoreAgentExtra,
    snapshotAgentExtra,
} from './agent-chat-message.js';
import {
    commitPreparedReasoning,
    createCommitReasoningState,
    prepareCommitReasoning,
    trackCommitReasoningEvent,
} from './agent-chat-commit-reasoning.js';
import { CHAT_COMMIT_REASON } from '../../../scripts/chat-payload-transport.js';

const TERMINAL_EVENTS = new Set(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);

export function attachHostCommitBridge({
    runId,
    chatRef = null,
    stableChatId = null,
    generationType = 'normal',
    safeInvoke,
    readWorkspaceFile,
    readModelTurn,
    subscribe,
    subscribeLiveProjection = null,
    loadScript = loadMainScript,
    persistChat = persistActiveChat,
}) {
    const normalizedRunId = requireRunId(runId);
    const state = {
        runId: normalizedRunId,
        chatRef,
        stableChatId,
        generationType: String(generationType || 'normal').trim() || 'normal',
        messageId: null,
        messageRef: null,
        swipeId: null,
        // `createdMessage` is captured when this run first owns a target so
        // a later `run_rollback_targets` can tell whether deleting the run means
        // removing the whole chat entry or only popping the swipe this run added
        // to a pre-existing assistant message (regenerate / swipe generation
        // types). See agent-run-message-rollback.js for the consumer.
        createdMessage: null,
        // Keep the raw chat target for this run. `append` adds the selected
        // file text as the next raw contribution, then cleanup runs on the
        // whole target so storage-time regex can match across commit chunks.
        rawCommittedText: '',
        commitSeq: 0,
        reasoning: createCommitReasoningState(),
        current: null,
        pendingFrame: null,
        liveMessageEventsEmitted: false,
        settlement: null,
        loadScript,
        persistChat,
        stop: null,
        stopLive: null,
    };
    const stop = subscribe(normalizedRunId, (event) => {
        trackCommitReasoningEvent(state.reasoning, event);
        if (event?.type === 'chat_commit_requested') {
            void handleChatCommitRequested({
                state,
                event,
                safeInvoke,
                readWorkspaceFile,
                readModelTurn,
            }).catch(reportAsyncError);
            return;
        }

        if (event?.type === 'persistent_state_metadata_update_requested') {
            void handlePersistentStateMetadataUpdateRequested({
                state,
                event,
                safeInvoke,
            }).catch(reportAsyncError);
            return;
        }

        if (TERMINAL_EVENTS.has(event?.type)) {
            void settleHostCommitBridge(state).catch(reportAsyncError);
        }
    }, { onError: reportAsyncError });

    state.stop = stop;
    if (typeof subscribeLiveProjection === 'function') {
        state.stopLive = subscribeLiveProjection(normalizedRunId, (update) => {
            applyLiveProjectionUpdate(state, update);
        }, { onError: reportAsyncError });
    }
    return state;
}

export function settleHostCommitBridge(state) {
    if (!state?.runId) {
        throw new Error('agent.host_commit_bridge_invalid: bridge state is required');
    }
    if (!state.settlement) {
        state.settlement = finalizeLivePartial(state)
            .finally(() => detachHostCommitBridge(state));
    }
    return state.settlement;
}

function detachHostCommitBridge(state) {
    if (typeof state.stop === 'function') {
        state.stop();
    }
    stopLiveProjection(state);
}

function reportAsyncError(error) {
    queueMicrotask(() => {
        throw error;
    });
}

async function handleChatCommitRequested({ state, event, safeInvoke, readWorkspaceFile, readModelTurn }) {
    const payload = event?.payload || {};
    const commitId = requirePayloadString(payload, 'commitId');

    let resolution;
    let isExplicit = false;
    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        await flushLiveWriteFrame(state);
        const path = requirePayloadString(payload, 'path');
        const mode = normalizeCommitMode(payload.mode);
        isExplicit = requirePayloadBoolean(payload, 'isExplicit');
        const file = await readWorkspaceFile({ runId: state.runId, path });
        if (file?.sha256 !== requirePayloadString(payload, 'sha256')) {
            throw new Error('agent.chat_commit_workspace_changed: workspace content changed before commit');
        }
        const reasoning = await prepareCommitReasoning(state.reasoning, state.runId, readModelTurn);
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
                reasoning: reasoning.delta,
            });
            messageId = getActiveMessageId(script.chat);
            captureMessageTarget(state, script.chat, messageId, lengthBefore);
        } else {
            messageId = Number(state.messageId);
            assertActiveAgentMessage(script.chat, state);
            // `getMessage` is the complete cleaned target, not an append
            // fragment. Use appendFinal to rewrite this run's chat message.
            const firstPublishedOutput = state.commitSeq === 0 && state.current != null;
            await script.saveReply({
                type: 'appendFinal',
                getMessage,
                reasoning: reasoning.delta,
                fromStreaming: firstPublishedOutput,
            });
            if (firstPublishedOutput) {
                await emitLiveMessageEventsOnce(state, script, messageId, payload.generationType);
            }
        }

        commitPreparedReasoning(state.reasoning, reasoning);
        state.rawCommittedText = rawCommittedText;
        const message = script.chat[messageId];
        const previousAgentExtra = snapshotAgentExtra(message);
        const nextCommitSeq = state.commitSeq + 1;
        mergeAgentCommitExtraIntoMessage(script.chat, messageId, payload, file, nextCommitSeq, {
            createdMessage: state.createdMessage,
            swipeId: state.swipeId,
        });
        try {
            await state.persistChat(script, CHAT_COMMIT_REASON.GENERATION_CHECKPOINT);
        } catch (error) {
            restoreAgentExtra(message, previousAgentExtra);
            throw error;
        }
        state.commitSeq = nextCommitSeq;
        resolution = { messageId: String(messageId) };
    } catch (error) {
        resolution = {
            error: String(error?.message ?? error),
        };
    }
    await safeInvoke('resolve_agent_chat_commit', {
        dto: { runId: state.runId, commitId, ...resolution },
    });
    if (!resolution.error) {
        state.current = null;
        if (isExplicit) {
            stopLiveProjection(state);
        }
    }
}

function applyLiveProjectionUpdate(state, update) {
    switch (update.type) {
        case 'snapshot': {
            for (const call of update.calls) {
                const write = liveWriteFromCall(call);
                if (write) {
                    state.current = write;
                }
            }
            scheduleLiveWriteFrame(state);
            return;
        }
        case 'replace': {
            const write = liveWriteFromCall(update.call);
            if (write) {
                state.current = write;
                scheduleLiveWriteFrame(state);
            }
            return;
        }
        case 'append': {
            if (!sameLiveCall(state.current, update)) return;
            if (update.field === 'path') {
                state.current.path += update.text;
            } else if (update.field === 'content') {
                state.current.content += update.text;
            } else {
                throw new Error(`agent.live_write_field_invalid: unsupported write field ${update.field}`);
            }
            scheduleLiveWriteFrame(state);
            return;
        }
        case 'remove':
            // Removal means the live projection no longer owns this call. It
            // does not revoke content the user has already received.
            return;
        default:
            throw new Error(`agent.live_update_invalid: unsupported update ${update.type}`);
    }
}

function liveWriteFromCall(call) {
    return call.toolId === 'builtin:workspace.write_file'
        && call.invocationExitPolicy === 'run_finish_allowed'
        ? call
        : null;
}

function sameLiveCall(current, update) {
    return current?.invocationId === update.invocationId
        && current?.toolCallIndex === update.toolCallIndex;
}

function scheduleLiveWriteFrame(state) {
    if (state.pendingFrame || !state.current) return;
    /** @type {{ id: number | null; release: (() => void) | null; promise: Promise<void> | null }} */
    const frame = { id: null, release: null, promise: null };
    const ready = new Promise(resolve => {
        frame.release = resolve;
        frame.id = requestAnimationFrame(() => {
            frame.id = null;
            resolve();
        });
    });
    frame.promise = ready
        .then(() => renderCurrentLiveWrite(state))
        .finally(() => {
            if (state.pendingFrame === frame) {
                state.pendingFrame = null;
            }
        });
    state.pendingFrame = frame;
    void frame.promise.catch(reportAsyncError);
}

async function flushLiveWriteFrame(state) {
    const frame = state.pendingFrame;
    if (frame?.id != null) {
        cancelAnimationFrame(frame.id);
        frame.id = null;
        frame.release?.();
    }
    if (frame?.promise) {
        await frame.promise;
    }
    await renderCurrentLiveWrite(state);
}

async function renderCurrentLiveWrite(state) {
    const current = state.current;
    if (!current
        || !isAutoCommitTextPath(current.path)) {
        return;
    }

    const script = await state.loadScript();
    await assertCurrentChat(state.chatRef, state.stableChatId);

    if (state.messageId == null) {
        const getMessage = prepareGeneratedReplyForDisplay(script, current.content, state.generationType);
        if (!getMessage) return;
        const lengthBefore = script.chat.length;
        await script.saveReply({
            type: initialCommitSaveType(state.generationType, 'replace'),
            getMessage,
            fromStreaming: true,
        });
        const messageId = getActiveMessageId(script.chat);
        captureMessageTarget(state, script.chat, messageId, lengthBefore);
    }

    const latest = state.current;
    if (!latest
        || !isAutoCommitTextPath(latest.path)) {
        return;
    }
    const getMessage = prepareGeneratedReplyForDisplay(script, latest.content, state.generationType);
    if (!getMessage) return;

    assertActiveAgentMessage(script.chat, state);
    const messageId = Number(state.messageId);
    const message = script.chat[messageId];
    if (message.mes === getMessage) return;
    message.mes = getMessage;
    if (typeof script.syncMesToSwipe !== 'function' || !script.syncMesToSwipe(messageId)) {
        throw new Error('agent.live_write_swipe_sync_failed: active swipe could not be updated');
    }
    if (typeof script.updateMessageBlock !== 'function') {
        throw new Error('updateMessageBlock is not available');
    }
    script.updateMessageBlock(messageId, message, { transient: true });
}

async function finalizeLivePartial(state) {
    if (!state.current) return;
    await flushLiveWriteFrame(state);
    if (state.messageId == null) return;

    await assertCurrentChat(state.chatRef, state.stableChatId);
    const script = await state.loadScript();
    assertActiveAgentMessage(script.chat, state);
    const messageId = Number(state.messageId);
    const firstPublishedOutput = state.commitSeq === 0;
    await script.saveReply({
        type: 'appendFinal',
        getMessage: String(script.chat[messageId].mes ?? ''),
        fromStreaming: firstPublishedOutput,
    });
    if (firstPublishedOutput) {
        await emitLiveMessageEventsOnce(state, script, messageId, state.generationType);
    }
    await state.persistChat(script, CHAT_COMMIT_REASON.GENERATION_CHECKPOINT);
    state.current = null;
}

async function emitLiveMessageEventsOnce(state, script, messageId, generationType) {
    if (state.liveMessageEventsEmitted) return;
    await emitGeneratedMessageEvents(script, messageId, generationType);
    state.liveMessageEventsEmitted = true;
}

function stopLiveProjection(state) {
    if (typeof state.stopLive === 'function') {
        state.stopLive();
        state.stopLive = null;
    }
    state.current = null;
}

async function handlePersistentStateMetadataUpdateRequested({ state, event, safeInvoke }) {
    const payload = event?.payload || {};
    const updateId = requirePayloadString(payload, 'updateId');

    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        const script = await state.loadScript();
        const messageId = normalizeMessageId(payload.messageId ?? state.messageId);
        const stateId = requirePayloadString(payload, 'stateId');
        mergePersistentStateExtraIntoMessage(script.chat, messageId, payload, stateId);
        await state.persistChat(script, CHAT_COMMIT_REASON.MUTATION);

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

function requirePayloadBoolean(payload, key) {
    const value = payload?.[key];
    if (typeof value !== 'boolean') {
        throw new Error(`agent.host_payload_invalid: ${key} must be a boolean`);
    }
    return value;
}
