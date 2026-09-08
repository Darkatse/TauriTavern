// @ts-check

import { assertCurrentChat } from './agent-chat-identity.js';
import { assertActiveAgentMessage } from './agent-chat-message.js';

export function captureHostPresentation(state, script, pendingWrite) {
    return {
        chatRef: state.chatRef,
        stableChatId: state.stableChatId,
        generationType: state.generationType,
        liveEnabled: state.liveEnabled,
        chatLength: state.messageId == null ? state.chatLength ?? script.chat.length : Number(state.messageId) + 1,
        messageId: state.messageId,
        swipeId: state.swipeId,
        createdMessage: state.createdMessage,
        rawCommittedText: state.rawCommittedText,
        commitSeq: state.commitSeq,
        reasoning: {
            commitInvocationIds: [...state.reasoning.commitInvocationIds],
            turns: state.reasoning.turns,
            cursor: state.reasoning.cursor,
        },
        pendingWrite,
        liveMessageEventsEmitted: state.liveMessageEventsEmitted,
    };
}

export async function restoreHostPresentation(runId, presentation, script, revision = false) {
    if (!presentation || typeof presentation !== 'object') {
        throw new Error('agent.resume_presentation_missing: this run has no saved chat presentation');
    }
    await assertCurrentChat(presentation.chatRef, presentation.stableChatId);
    if (revision) {
        const message = script.chat.at(-1);
        if (message?.extra?.tauritavern?.agent?.runId !== runId) {
            throw new Error('agent.resume_message_changed: the selected message belongs to another run');
        }
        presentation = {
            ...presentation,
            chatLength: script.chat.length,
            messageId: script.chat.length - 1,
            swipeId: message.swipe_id,
            rawCommittedText: message.mes,
        };
    }
    if (script.chat.length !== presentation.chatLength) {
        throw new Error('agent.resume_chat_changed: the chat has advanced since this run stopped');
    }
    const messageRef = presentation.messageId == null ? null : script.chat[presentation.messageId];
    const restored = {
        ...presentation,
        messageRef,
        reasoning: {
            ...presentation.reasoning,
            commitInvocationIds: new Set(presentation.reasoning.commitInvocationIds),
        },
    };
    if (messageRef) {
        assertActiveAgentMessage(script.chat, restored);
        if (messageRef.extra?.tauritavern?.agent?.runId !== runId) {
            throw new Error('agent.resume_message_changed: the selected message belongs to another run');
        }
    } else if (presentation.messageId != null) {
        throw new Error('agent.resume_message_missing: this run\'s chat message no longer exists');
    }
    return restored;
}

export async function finishHostPresentation(state, finalizePartial, detach) {
    if (!state.presentationCheckpoint) {
        state.stopLive?.();
        state.stopLive = null;
        // Commit acknowledgement failures are already reported by the bridge;
        // the runtime owns whether their effects are safe to resume.
        await state.pendingMutation.catch(() => {});
        const script = await state.loadScript();
        const pendingWrite = state.current ? structuredClone(state.current) : state.pendingWrite ?? null;
        if (state.messageId != null) {
            const saved = captureHostPresentation(state, script, pendingWrite);
            state.messageRef = (await restoreHostPresentation(state.runId, saved, script)).messageRef;
        }
        await finalizePartial(state);
        state.presentationCheckpoint = captureHostPresentation(state, script, pendingWrite);
    }
    // A failed checkpoint publication retries these same bytes of presentation,
    // without repeating chat writes, regex cleanup or message events.
    if (state.finishPresentation) {
        await state.finishPresentation({
            runId: state.runId,
            terminalSeq: state.terminalEvent.seq,
            presentation: state.presentationCheckpoint,
        });
    }
    detach(state);
}
