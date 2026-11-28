// @ts-check

import { extractErrorText } from '../../../scripts/util/command-error-utils.js';
import { assertCurrentChat } from './agent-chat-identity.js';
import {
    assertActiveAgentMessage,
    captureMessageTarget,
    getActiveMessageId,
    initialCommitSaveType,
    mergeAgentCommitExtraIntoMessage,
    mergePersistentStateExtraIntoMessage,
    normalizeCommitMode,
    persistActiveChat,
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
import { finishHostPresentation } from './agent-chat-presentation-checkpoint.js';
import {
    applyLiveProjectionUpdate,
    createLiveWriteScheduler,
    endLiveWrite,
    finalizeLiveMessage,
    finalizeLivePartial,
    flushLiveWriteFrame,
    stopLiveProjection,
} from './agent-chat-live-write.js';

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
    scheduleRender = createLiveWriteScheduler(),
    presentation = null,
    chatLength = null,
    finishPresentation = null,
}) {
    const normalizedRunId = requireRunId(runId);
    let resolveTerminal;
    const terminal = new Promise(resolve => { resolveTerminal = resolve; });
    const state = {
        runId: normalizedRunId,
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
        // The projected write shown in chat. It stays after its projection ends,
        // until a commit or the terminal save takes it over.
        liveWrite: null,
        // The projected call whose appends still extend `liveWrite`.
        liveCall: null,
        // Created on the first render; see createGeneratedReplyPreview.
        preview: null,
        pendingWrite: null,
        pendingFrame: null,
        liveMessageEventsEmitted: false,
        settlement: null,
        presentationCheckpoint: null,
        chatLength,
        loadScript,
        persistChat,
        scheduleRender,
        liveEnabled: typeof subscribeLiveProjection === 'function',
        finishPresentation,
        terminalEvent: null,
        terminal,
        pendingMutation: Promise.resolve(),
        stop: null,
        stopLive: null,
        ...presentation,
        chatRef,
        stableChatId,
        generationType: String(generationType || 'normal').trim() || 'normal',
    };
    const stop = subscribe(normalizedRunId, (event) => {
        trackCommitReasoningEvent(state.reasoning, event);
        if (event?.type === 'chat_commit_requested') {
            state.pendingMutation = handleChatCommitRequested({
                state,
                event,
                safeInvoke,
                readWorkspaceFile,
                readModelTurn,
            });
            void state.pendingMutation.catch(reportAsyncError);
            return;
        }

        if (event?.type === 'persistent_state_metadata_update_requested') {
            state.pendingMutation = handlePersistentStateMetadataUpdateRequested({
                state,
                event,
                safeInvoke,
            });
            void state.pendingMutation.catch(reportAsyncError);
            return;
        }

        if (TERMINAL_EVENTS.has(event?.type)) {
            state.terminalEvent = event;
            resolveTerminal();
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
        // Another poller may see the terminal first. Finish only after this
        // bridge has consumed all preceding commit and reasoning events.
        state.settlement = (state.terminalEvent ? Promise.resolve() : state.terminal)
            .then(() => finishHostPresentation(state, finalizeLivePartial, detachHostCommitBridge))
            .catch(error => {
                state.settlement = null;
                throw error;
            });
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
            const firstPublishedOutput = state.commitSeq === 0 && state.liveWrite != null;
            await script.saveReply({
                type: 'appendFinal',
                getMessage,
                reasoning: reasoning.delta,
                fromStreaming: firstPublishedOutput,
            });
            if (firstPublishedOutput) {
                await finalizeLiveMessage(state, script, messageId, payload.generationType);
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
            error: extractErrorText(error),
        };
    }
    if (!resolution.error) {
        endLiveWrite(state);
        state.pendingWrite = null;
        if (isExplicit) {
            stopLiveProjection(state);
        }
    }
    await safeInvoke('resolve_agent_chat_commit', {
        dto: { runId: state.runId, commitId, ...resolution },
    });
}

async function handlePersistentStateMetadataUpdateRequested({ state, event, safeInvoke }) {
    const payload = event?.payload || {};
    const updateId = requirePayloadString(payload, 'updateId');

    try {
        await assertCurrentChat(payload.chatRef, payload.stableChatId);
        const script = await state.loadScript();
        assertActiveAgentMessage(script.chat, state);
        const messageId = Number(state.messageId);
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
                error: extractErrorText(error),
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
