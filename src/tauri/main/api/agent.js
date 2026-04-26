// @ts-check

import { buildAgentPromptSnapshot } from './agent-prompt-snapshot.js';

const DEFAULT_EVENT_POLL_MS = 500;

/**
 * @typedef {{ kind: 'character'; characterId: string; fileName: string }} CharacterChatRef
 * @typedef {{ kind: 'group'; chatId: string }} GroupChatRef
 * @typedef {CharacterChatRef | GroupChatRef} ChatRef
 */

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
function createAgentApi({ safeInvoke }) {
    async function startRunWithPromptSnapshot(input) {
        const dto = await normalizePromptSnapshotRunInput(input);
        return safeInvoke('start_agent_run', { dto });
    }

    async function startRunFromLegacyGenerate(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent startRunFromLegacyGenerate input must be an object');
        }

        const generationType = normalizeGenerationType(input.generationType);
        const agentOptions = normalizePhase2aAgentOptions(input.options);
        const snapshot = await buildAgentPromptSnapshot({
            generationType,
            generateOptions: input.generateOptions,
        });

        return startRunWithPromptSnapshot({
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            generationType,
            profileId: input.profileId,
            promptSnapshot: snapshot.promptSnapshot,
            generationIntent: mergePlainObject(snapshot.generationIntent, input.generationIntent),
            options: agentOptions,
        });
    }

    async function cancel(runId) {
        const normalizedRunId = requireRunId(runId);
        return safeInvoke('cancel_agent_run', { dto: { runId: normalizedRunId } });
    }

    async function readEvents(input) {
        const runId = requireRunId(input?.runId);
        return safeInvoke('read_agent_run_events', {
            dto: {
                runId,
                afterSeq: input?.afterSeq,
                beforeSeq: input?.beforeSeq,
                limit: input?.limit,
            },
        });
    }

    async function readWorkspaceFile(input) {
        const runId = requireRunId(input?.runId);
        const path = String(input?.path || '').trim();
        if (!path) {
            throw new Error('path is required');
        }

        return safeInvoke('read_agent_workspace_file', { dto: { runId, path } });
    }

    function subscribe(runId, handler, options = {}) {
        const normalizedRunId = requireRunId(runId);
        if (typeof handler !== 'function') {
            throw new Error('handler is required');
        }

        const intervalMs = normalizePollInterval(options?.intervalMs);
        let afterSeq = Number(options?.afterSeq || 0);
        let stopped = false;
        let timer = null;

        const tick = async () => {
            if (stopped) {
                return;
            }

            try {
                const result = await readEvents({
                    runId: normalizedRunId,
                    afterSeq,
                    limit: options?.limit || 100,
                });
                const events = Array.isArray(result?.events) ? result.events : [];
                for (const event of events) {
                    afterSeq = Math.max(afterSeq, Number(event?.seq || 0));
                    handler(event);
                }
            } catch (error) {
                if (typeof options?.onError === 'function') {
                    options.onError(error);
                } else {
                    queueMicrotask(() => {
                        throw error;
                    });
                }
            } finally {
                if (!stopped) {
                    timer = setTimeout(tick, intervalMs);
                }
            }
        };

        timer = setTimeout(tick, 0);

        return function unsubscribe() {
            stopped = true;
            if (timer !== null) {
                clearTimeout(timer);
                timer = null;
            }
        };
    }

    async function commit(input) {
        const runId = requireRunId(input?.runId);
        const draft = await safeInvoke('prepare_agent_run_commit', { dto: { runId } });
        await assertCurrentChat(draft?.chatRef, draft?.stableChatId);

        const script = await import('../../../script.js');
        if (typeof script.saveReply !== 'function') {
            throw new Error('saveReply is not available');
        }

        await script.saveReply({
            type: draft?.generationType || 'normal',
            getMessage: String(draft?.message?.mes ?? ''),
        });

        const messageId = mergeAgentExtraIntoActiveMessage(script.chat, draft?.message?.extra);
        await persistActiveChat(script);

        return safeInvoke('finalize_agent_run_commit', {
            dto: {
                runId,
                messageId: String(input?.messageId ?? messageId),
            },
        });
    }

    return {
        startRunWithPromptSnapshot,
        startRunFromLegacyGenerate,
        cancel,
        readEvents,
        readWorkspaceFile,
        subscribe,
        commit,
        prepareCommit(input) {
            const runId = requireRunId(input?.runId);
            return safeInvoke('prepare_agent_run_commit', { dto: { runId } });
        },
        finalizeCommit(input) {
            const runId = requireRunId(input?.runId);
            return safeInvoke('finalize_agent_run_commit', {
                dto: {
                    runId,
                    messageId: input?.messageId,
                },
            });
        },
        approveToolCall() {
            throw new Error('approveToolCall is not implemented in Agent Phase 2A');
        },
        listRuns() {
            throw new Error('listRuns is not implemented in Agent Phase 2A');
        },
        readDiff() {
            throw new Error('readDiff is not implemented in Agent Phase 2A');
        },
        rollback() {
            throw new Error('rollback is not implemented in Agent Phase 2A');
        },
    };
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function normalizePhase2aAgentOptions(value) {
    if (value != null && !isPlainObject(value)) {
        throw new Error('agent.options_invalid: options must be an object');
    }

    const options = value || {};
    if (options.stream === true) {
        throw new Error('agent.phase2a_stream_unsupported: Agent Phase 2A only supports non-streaming model calls');
    }
    if (options.autoCommit === true) {
        throw new Error('agent.phase2a_auto_commit_unsupported: commit is owned by the frontend adapter in Agent Phase 2A');
    }

    return {
        ...options,
        stream: false,
        autoCommit: false,
    };
}

async function normalizePromptSnapshotRunInput(input) {
    if (!input || typeof input !== 'object') {
        throw new Error('Agent startRunWithPromptSnapshot input is required');
    }

    const chatRef = input.chatRef || window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
    if (!chatRef || typeof chatRef !== 'object') {
        throw new Error('chatRef is required');
    }

    const stableChatId = String(input.stableChatId || '').trim() || await resolveStableChatId(chatRef);
    if (!stableChatId) {
        throw new Error('stableChatId is required');
    }

    return {
        ...input,
        chatRef,
        stableChatId,
    };
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

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function normalizePollInterval(value) {
    const intervalMs = Number(value || DEFAULT_EVENT_POLL_MS);
    if (!Number.isFinite(intervalMs) || intervalMs < 100) {
        return DEFAULT_EVENT_POLL_MS;
    }
    return intervalMs;
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

/**
 * @param {ChatRef | null | undefined} a
 * @param {ChatRef | null | undefined} b
 */
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

function mergeAgentExtraIntoActiveMessage(chat, extra) {
    if (!Array.isArray(chat) || chat.length === 0) {
        throw new Error('Cannot commit agent output because the active chat is empty');
    }

    const messageId = chat.length - 1;
    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('Cannot commit agent output because the active message is invalid');
    }

    message.extra = mergePlainObject(message.extra, extra);

    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }

    return messageId;
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

/**
 * @param {any} context
 */
export function installAgentApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.agent = createAgentApi({ safeInvoke });
}
