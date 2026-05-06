// @ts-check

import { buildAgentPromptSnapshot } from './agent-prompt-snapshot.js';
import { attachHostCommitBridge } from './agent-chat-commit-bridge.js';

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
        const handle = await safeInvoke('start_agent_run', { dto });
        attachHostCommitBridge({ runId: handle?.runId, safeInvoke, readWorkspaceFile, subscribe });
        return handle;
    }

    async function startRunFromLegacyGenerate(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent startRunFromLegacyGenerate input must be an object');
        }

        const generationType = normalizeGenerationType(input.generationType);
        const agentOptions = normalizeAgentRunOptions(input.options, input.presentation);
        const snapshot = await buildAgentPromptSnapshot({
            generationType,
            generateOptions: input.generateOptions,
        });

        return startRunWithPromptSnapshot({
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            generationType,
            profileId: input.profileId,
            persistBaseStateId: input.persistBaseStateId,
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

    async function readModelTurn(input) {
        const runId = requireRunId(input?.runId);
        const round = Number(input?.round);
        if (!Number.isInteger(round) || round <= 0) {
            throw new Error('round must be a positive integer');
        }
        const maxChars = input?.maxChars == null ? undefined : Number(input.maxChars);
        if (maxChars != null && (!Number.isInteger(maxChars) || maxChars <= 0)) {
            throw new Error('maxChars must be a positive integer');
        }

        return safeInvoke('read_agent_model_turn', {
            dto: {
                runId,
                round,
                ...(maxChars == null ? {} : { maxChars }),
            },
        });
    }

    async function pruneChatPersistentStates(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent pruneChatPersistentStates input must be an object');
        }
        const chatRef = input.chatRef || window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
        if (!chatRef || typeof chatRef !== 'object') {
            throw new Error('chatRef is required');
        }
        const stableChatId = String(input.stableChatId || '').trim() || await resolveStableChatId(chatRef);
        if (!stableChatId) {
            throw new Error('stableChatId is required');
        }

        return safeInvoke('prune_agent_chat_persistent_states', {
            dto: {
                chatRef,
                stableChatId,
            },
        });
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

    async function listProfiles() {
        return safeInvoke('list_agent_profiles');
    }

    async function listToolSpecs() {
        return safeInvoke('list_agent_tool_specs');
    }

    async function loadProfile(input) {
        const profileId = requireProfileId(input?.profileId ?? input?.profile_id ?? input);
        return safeInvoke('load_agent_profile', { dto: { profileId } });
    }

    async function saveProfile(input) {
        const profile = input?.profile ?? input;
        if (!isPlainObject(profile)) {
            throw new Error('agent.profile_required: profile must be an object');
        }
        return safeInvoke('save_agent_profile', { dto: { profile } });
    }

    async function deleteProfile(input) {
        const profileId = requireProfileId(input?.profileId ?? input?.profile_id ?? input);
        return safeInvoke('delete_agent_profile', { dto: { profileId } });
    }

    return {
        startRunWithPromptSnapshot,
        startRunFromLegacyGenerate,
        cancel,
        readEvents,
        readWorkspaceFile,
        readModelTurn,
        pruneChatPersistentStates,
        subscribe,
        profiles: {
            list: listProfiles,
            load: loadProfile,
            save: saveProfile,
            delete: deleteProfile,
        },
        tools: {
            list: listToolSpecs,
        },
        approveToolCall() {
            throw new Error('approveToolCall is not implemented in Agent Phase 2B');
        },
        listRuns() {
            throw new Error('listRuns is not implemented in Agent Phase 2B');
        },
        readDiff() {
            throw new Error('readDiff is not implemented in Agent Phase 2B');
        },
        rollback() {
            throw new Error('rollback is not implemented in Agent Phase 2B');
        },
    };
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function normalizeAgentRunOptions(value, presentationOverride = undefined) {
    if (value != null && !isPlainObject(value)) {
        throw new Error('agent.options_invalid: options must be an object');
    }

    const options = value || {};
    if (options.stream === true) {
        throw new Error('agent.phase2b_stream_unsupported: Agent Phase 2B only supports non-streaming model calls');
    }
    if (Object.prototype.hasOwnProperty.call(options, 'autoCommit')) {
        throw new Error('agent.auto_commit_removed: Agent chat commits are driven by workspace.commit');
    }
    const presentation = normalizeAgentRunPresentation(presentationOverride ?? options.presentation);

    return {
        ...options,
        stream: false,
        ...(presentation ? { presentation } : {}),
    };
}

function normalizeAgentRunPresentation(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const presentation = String(value).trim();
    if (presentation !== 'foreground' && presentation !== 'background') {
        throw new Error('agent.presentation_invalid: presentation must be foreground or background');
    }
    return presentation;
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
        persistBaseStateId: normalizeOptionalString(input.persistBaseStateId),
        options: normalizeAgentRunOptions(input.options, input.presentation),
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

function requireProfileId(value) {
    const profileId = String(value || '').trim();
    if (!profileId) {
        throw new Error('profileId is required');
    }
    return profileId;
}

function normalizeOptionalString(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const text = String(value).trim();
    return text || undefined;
}

function normalizePollInterval(value) {
    const intervalMs = Number(value || DEFAULT_EVENT_POLL_MS);
    if (!Number.isFinite(intervalMs) || intervalMs < 100) {
        return DEFAULT_EVENT_POLL_MS;
    }
    return intervalMs;
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
