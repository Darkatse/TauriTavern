// @ts-check

import { buildAgentPromptSnapshotSeed } from './agent-prompt-snapshot.js';
import { assemblePromptSnapshotForProfile, createPromptAssemblyApi } from './agent-prompt-assembly-run.js';
import { attachHostCommitBridge } from './agent-chat-commit-bridge.js';
import { attachHostPromptAssemblyBridge } from './agent-prompt-assembly-bridge.js';
import { createAgentProfilesApi } from './agent-profiles.js';
import { createSharedRunEventSubscribe } from './agent-run-event-subscription.js';
import { normalizeAgentRunOptions } from './agent-run-options.js';
import { DEFAULT_AGENT_PROFILE_ID } from '../../../scripts/tauritavern/agent/agent-system-settings.js';

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
    const promptAssembly = createPromptAssemblyApi({ safeInvoke });
    const profiles = createAgentProfilesApi({ safeInvoke });

    async function startRunWithPromptSnapshot(input) {
        const dto = await normalizePromptSnapshotRunInput(input, { safeInvoke });
        const handle = await safeInvoke('start_agent_run', { dto });
        const hostSubscribe = createSharedRunEventSubscribe(handle?.runId, subscribe);
        attachHostCommitBridge({
            runId: handle?.runId,
            safeInvoke,
            readWorkspaceFile,
            subscribe: hostSubscribe,
        });
        attachHostPromptAssemblyBridge({
            runId: handle?.runId,
            safeInvoke,
            promptAssembly,
            subscribe: hostSubscribe,
        });
        return handle;
    }

    async function startRunFromLegacyGenerate(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent startRunFromLegacyGenerate input must be an object');
        }

        const generationType = normalizeGenerationType(input.generationType);
        const agentOptions = normalizeAgentRunOptions(input.options, input.presentation);
        const legacySnapshot = await buildAgentPromptSnapshotSeed({
            generationType,
            generateOptions: input.generateOptions,
            profileId: input.profileId,
        });
        const snapshot = await assemblePromptSnapshotForProfile({
            generationType,
            profileId: input.profileId,
            jsonSchema: input.generateOptions?.jsonSchema ?? null,
            promptSnapshotResult: legacySnapshot,
            promptAssembly,
        });

        return startRunWithPromptSnapshot({
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            generationType,
            profileId: input.profileId,
            persistBaseStateId: input.persistBaseStateId,
            promptSnapshot: snapshot.promptSnapshot,
            frozenRunInputSnapshot: snapshot.frozenRunInputSnapshot,
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
                ...(input?.includeTimelineProjection === true ? { includeTimelineProjection: true } : {}),
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
        const invocationId = String(input?.invocationId || '').trim();

        return safeInvoke('read_agent_model_turn', {
            dto: {
                runId,
                round,
                ...(invocationId ? { invocationId } : {}),
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

    async function listToolSpecs() {
        return safeInvoke('list_agent_tool_specs');
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
        profiles,
        tools: {
            list: listToolSpecs,
        },
        promptAssembly,
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

async function normalizePromptSnapshotRunInput(input, { safeInvoke }) {
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

    const skillScopeRefs = await resolveSkillScopeRefsForRun(input, safeInvoke);

    return {
        ...input,
        chatRef,
        stableChatId,
        skillScopeRefs,
        persistBaseStateId: normalizeOptionalString(input.persistBaseStateId),
        options: normalizeAgentRunOptions(input.options, input.presentation),
    };
}

async function resolveSkillScopeRefsForRun(input, safeInvoke) {
    const refs = normalizeSkillScopeRefs(input.skillScopeRefs ?? input.skill_scope_refs);
    if (refs.preset) {
        return refs;
    }

    const profile = await loadRunProfile(input.profileId ?? input.profile_id, safeInvoke);
    if (profile?.preset?.mode !== 'currentPromptSnapshot') {
        return refs;
    }

    return {
        ...refs,
        preset: resolveCurrentPresetRef(),
    };
}

async function loadRunProfile(profileId, safeInvoke) {
    const resolvedProfileId = normalizeOptionalString(profileId) || DEFAULT_AGENT_PROFILE_ID;
    const result = await safeInvoke('load_agent_profile', {
        dto: {
            profileId: resolvedProfileId,
        },
    });
    const profile = result?.profile;
    if (!profile) {
        throw new Error(`agent.profile_not_found: Agent Profile '${resolvedProfileId}' was not found`);
    }
    return profile;
}

function normalizeSkillScopeRefs(value) {
    if (value == null) {
        return {};
    }
    if (!isPlainObject(value)) {
        throw new Error('agent.skill_scope_refs_invalid: skillScopeRefs must be an object');
    }

    const refs = {};
    const preset = normalizeOptionalPresetRef(value.preset);
    if (preset) {
        refs.preset = preset;
    }
    const characterId = normalizeOptionalString(value.characterId ?? value.character_id);
    if (characterId) {
        refs.characterId = characterId;
    }
    return refs;
}

function normalizeOptionalPresetRef(value) {
    if (value == null) {
        return undefined;
    }
    if (!isPlainObject(value)) {
        throw new Error('agent.skill_scope_refs_preset_invalid: skillScopeRefs.preset must be an object');
    }
    const apiId = normalizeOptionalString(value.apiId ?? value.api_id);
    const name = normalizeOptionalString(value.name);
    if (!apiId || !name) {
        throw new Error('agent.skill_scope_refs_preset_invalid: skillScopeRefs.preset requires apiId and name');
    }
    return { apiId, name };
}

function resolveCurrentPresetRef() {
    const context = window.SillyTavern?.getContext?.();
    if (!context || typeof context !== 'object') {
        throw new Error('agent.current_preset_context_unavailable: SillyTavern context is required to resolve the current preset');
    }
    const presetManager = context.getPresetManager?.();
    if (!presetManager) {
        throw new Error('agent.current_preset_manager_unavailable: current preset manager is unavailable');
    }

    const selectedPreset = String(presetManager.getSelectedPreset?.() ?? '').trim();
    if (selectedPreset === 'gui') {
        throw new Error('agent.current_preset_unsaved: CurrentPromptSnapshot Agent runs require a saved preset to resolve preset-scoped Skills');
    }

    const apiId = normalizeOptionalString(presetManager.apiId);
    const name = normalizeOptionalString(presetManager.getSelectedPresetName?.());
    if (!apiId || !name) {
        throw new Error('agent.current_preset_ref_invalid: current preset did not resolve apiId and name');
    }

    return { apiId, name };
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
