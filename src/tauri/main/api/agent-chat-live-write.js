// @ts-check

import { createStreamingRenderScheduler } from '../../../scripts/tauri/perf/streaming-render-policy.js';
import { mustGetSillyTavernContext } from '../adapters/st/active-chat-ref.js';
import { assertCurrentChat } from './agent-chat-identity.js';
import {
    assertActiveAgentMessage,
    captureMessageTarget,
    createGeneratedReplyPreview,
    finalizeGeneratedMessage,
    getActiveMessageId,
    initialCommitSaveType,
    isAutoCommitTextPath,
} from './agent-chat-message.js';
import { CHAT_COMMIT_REASON } from '../../../scripts/chat-payload-transport.js';

// The chat preview of a foreground run's live write_file content, on the commit bridge state.

/** Paces one run's preview renders by the user's streaming FPS. */
export function createLiveWriteScheduler() {
    return createStreamingRenderScheduler(() => mustGetSillyTavernContext().powerUserSettings.streaming_fps);
}

export function applyLiveProjectionUpdate(state, update) {
    switch (update.type) {
        case 'snapshot': {
            state.liveCall = null;
            for (const call of update.calls) {
                acceptLiveCall(state, call);
            }
            scheduleLiveWriteFrame(state);
            return;
        }
        case 'replace': {
            if (acceptLiveCall(state, update.call)) {
                scheduleLiveWriteFrame(state);
            }
            return;
        }
        case 'append': {
            const call = state.liveCall;
            if (!sameLiveCall(call, update)) return;
            if (update.field === 'path') {
                call.path += update.text;
            } else if (update.field === 'content') {
                call.content += update.text;
            } else {
                throw new Error(`agent.live_write_field_invalid: unsupported write field ${update.field}`);
            }
            scheduleLiveWriteFrame(state);
            return;
        }
        case 'remove':
            // Removal ends the call, not the content the user has already received.
            if (sameLiveCall(state.liveCall, update)) {
                state.liveCall = null;
            }
            return;
        case 'responseReplace':
        case 'responseAppend':
        case 'responseRemove':
            return;
        default:
            throw new Error(`agent.live_update_invalid: unsupported update ${update.type}`);
    }
}

/** @returns {boolean} Whether the call is a write the chat now shows */
function acceptLiveCall(state, call) {
    if (call.toolId === 'builtin:workspace.write_file'
        && call.invocationExitPolicy === 'run_finish_allowed') {
        state.liveWrite = call;
        state.liveCall = call;
        return true;
    }
    if (sameLiveCall(state.liveCall, call)) {
        state.liveCall = null;
    }
    return false;
}

export function endLiveWrite(state) {
    state.liveWrite = null;
    state.liveCall = null;
}

function sameLiveCall(call, update) {
    return call?.invocationId === update.invocationId
        && call?.toolCallIndex === update.toolCallIndex;
}

function scheduleLiveWriteFrame(state) {
    if (state.pendingFrame || !state.liveWrite) return;
    /** `cancel` is set until the render starts. @type {{ cancel: (() => void) | null; promise: Promise<void> | null }} */
    const frame = { cancel: null, promise: null };
    state.pendingFrame = frame;
    frame.cancel = state.scheduleRender(() => {
        frame.cancel = null;
        frame.promise = renderCurrentLiveWrite(state).finally(() => {
            if (state.pendingFrame === frame) {
                state.pendingFrame = null;
            }
        });
        void frame.promise.catch(reportAsyncError);
        return frame.promise;
    });
}

/**
 * Cancels the frame if its render has not started.
 * @returns {Promise<void> | null} The render in progress, if any.
 */
function cancelPendingLiveWriteFrame(state) {
    const frame = state.pendingFrame;
    if (!frame) return null;
    if (!frame.cancel) return frame.promise;
    frame.cancel();
    state.pendingFrame = null;
    return null;
}

export async function flushLiveWriteFrame(state) {
    await cancelPendingLiveWriteFrame(state);
    await renderCurrentLiveWrite(state);
}

async function renderCurrentLiveWrite(state) {
    const current = state.liveWrite;
    if (!current
        || !isAutoCommitTextPath(current.path)) {
        return;
    }

    const script = await state.loadScript();
    await assertCurrentChat(state.chatRef, state.stableChatId);
    state.preview ??= createGeneratedReplyPreview(script, state.generationType);

    if (state.messageId == null) {
        const getMessage = state.preview.text(current.content);
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

    const latest = state.liveWrite;
    if (!latest
        || !isAutoCommitTextPath(latest.path)) {
        return;
    }
    const text = state.preview.text(latest.content);
    if (!text) return;

    assertActiveAgentMessage(script.chat, state);
    const messageId = Number(state.messageId);
    const message = script.chat[messageId];
    if (message.mes !== text) {
        message.mes = text;
        if (typeof script.syncMesToSwipe !== 'function' || !script.syncMesToSwipe(messageId)) {
            throw new Error('agent.live_write_swipe_sync_failed: active swipe could not be updated');
        }
    }
    state.preview.write(messageId, text);
}

export async function finalizeLivePartial(state) {
    if (!state.liveWrite) return;
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
        await finalizeLiveMessage(state, script, messageId, state.generationType);
    }
    await state.persistChat(script, CHAT_COMMIT_REASON.GENERATION_CHECKPOINT);
    endLiveWrite(state);
}

export async function finalizeLiveMessage(state, script, messageId, generationType) {
    if (state.liveMessageEventsEmitted) return script.finalizeMessageContent(messageId);
    await finalizeGeneratedMessage(script, messageId, generationType);
    state.liveMessageEventsEmitted = true;
}

export function stopLiveProjection(state) {
    state.liveEnabled = false;
    if (typeof state.stopLive === 'function') {
        state.stopLive();
        state.stopLive = null;
    }
    cancelPendingLiveWriteFrame(state);
    endLiveWrite(state);
}

function reportAsyncError(error) {
    queueMicrotask(() => {
        throw error;
    });
}
