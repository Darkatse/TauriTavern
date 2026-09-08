// @ts-check

import { buildAgentPromptSnapshotSeed } from './agent-prompt-snapshot.js';
import { assemblePromptSnapshotForProfile, createPromptAssemblyApi } from './agent-prompt-assembly-run.js';
import { attachHostCommitBridge, settleHostCommitBridge } from './agent-chat-commit-bridge.js';
import { attachHostPromptAssemblyBridge } from './agent-prompt-assembly-bridge.js';
import { resolveStableChatId } from './agent-chat-identity.js';
import { createAgentProfilesApi } from './agent-profiles.js';
import { createSharedRunEventSubscribe } from './agent-run-event-subscription.js';
import { createAgentRunLiveSubscribe } from './agent-run-live-subscription.js';
import { normalizeAgentRunOptions } from './agent-run-options.js';
import { createAgentRunRuntimeApi } from './agent-run-runtime.js';
import { confirmEmptyAgentPersist } from '../adapters/st/agent-empty-persist-popup.js';
import { DEFAULT_AGENT_PROFILE_ID } from '../../../scripts/tauritavern/agent/agent-system-settings.js';
import { ensureModelTargetLlmConnectionForProfile } from '../../../scripts/tauritavern/agent/model-target-llm-connection.js';
import { restoreHostPresentation } from './agent-chat-presentation-checkpoint.js';

/**
 * @typedef {{ kind: 'character'; characterId: string; fileName: string }} CharacterChatRef
 * @typedef {{ kind: 'group'; chatId: string }} GroupChatRef
 * @typedef {CharacterChatRef | GroupChatRef} ChatRef
 */

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any>; loadScript?: () => Promise<any> }} deps
 */
export function createAgentApi({ safeInvoke, loadScript = () => import('../../../script.js') }) {
    const promptAssembly = createPromptAssemblyApi({ safeInvoke });
    const profiles = createAgentProfilesApi({ safeInvoke });
    const runtime = createAgentRunRuntimeApi({ safeInvoke });
    const subscribeLiveProjection = createAgentRunLiveSubscribe({ safeInvoke });
    const commitBridges = new Map();

    async function startRunWithPromptSnapshot(input) {
        return startRunWithPromptSnapshotInternal(input, { ensureModelTargetConnection: true });
    }

    async function startRunWithPromptSnapshotInternal(input, { ensureModelTargetConnection, runProfile = null } = {}) {
        const dto = await normalizePromptSnapshotRunInput(input, {
            safeInvoke,
            ensureModelTargetConnection,
            runProfile,
        });
        const chatLength = (await loadScript()).chat.length;
        let handle;
        try {
            handle = await safeInvoke('start_agent_run', { dto });
        } catch (error) {
            // Only a missing inherited version can be replaced by an explicit empty start.
            if (!/^(?:Bad request: |Not found: )?agent\.(?:persist_state_missing|persistent_state_not_found):/u.test(error.message)) {
                throw error;
            }
            if (!await confirmEmptyAgentPersist()) {
                throw new DOMException('Agent run cancelled by user', 'AbortError');
            }
            dto.persistBaseStateId = undefined;
            dto.options.startWithEmptyPersist = true;
            handle = await safeInvoke('start_agent_run', { dto });
        }
        attachRunBridges(handle, dto, null, chatLength);
        return handle;
    }

    function attachRunBridges(handle, dto, presentation = null, chatLength = presentation?.chatLength) {
        const hostSubscribe = createSharedRunEventSubscribe(handle.runId, runtime.subscribe, handle.afterSeq);
        const commitBridge = attachHostCommitBridge({
            runId: handle.runId,
            chatRef: dto.chatRef,
            stableChatId: dto.stableChatId,
            generationType: dto.generationType,
            safeInvoke,
            readWorkspaceFile: runtime.readWorkspaceFile,
            readModelTurn: runtime.readModelTurn,
            subscribe: hostSubscribe,
            loadScript,
            presentation,
            chatLength,
            finishPresentation: async dto => {
                await safeInvoke('finish_agent_run_presentation', { dto });
                commitBridges.delete(handle.runId);
            },
            subscribeLiveProjection: dto.options?.stream !== false
                && dto.options?.presentation === 'foreground'
                ? subscribeLiveProjection
                : null,
        });
        commitBridges.set(handle.runId, commitBridge);
        attachHostPromptAssemblyBridge({
            runId: handle?.runId,
            safeInvoke,
            promptAssembly,
            subscribe: hostSubscribe,
        });
    }

    async function readCheckpoint(runId) {
        if (typeof runId !== 'string' || !runId.trim()) throw new Error('runId is required');
        runId = runId.trim();
        const pending = commitBridges.get(runId);
        if (pending?.terminalEvent) await settleHostCommitBridge(pending);
        return safeInvoke('read_agent_run_checkpoint', { dto: { runId } });
    }

    async function resume({ runId, additionalRounds = 0, checkpoint = null } = {}) {
        runId = String(runId || '').trim();
        if (!Number.isInteger(additionalRounds) || additionalRounds < 0) {
            throw new Error('agent.resume_rounds_invalid: additionalRounds must be a non-negative integer');
        }
        checkpoint ??= await readCheckpoint(runId);
        if (checkpoint.run.runId !== runId) throw new Error('agent.resume_checkpoint_mismatch: checkpoint belongs to another run');
        if (checkpoint.blockedReason || checkpoint.nextStep === 'finished') {
            throw new Error(`agent.resume_unavailable: ${checkpoint.blockedReason || 'this run has already finished'}`);
        }
        const presentation = await restoreHostPresentation(runId, checkpoint.presentation, await loadScript());
        const chatRef = window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
        const stableChatId = await resolveStableChatId(chatRef);
        const handle = await safeInvoke('resume_agent_run', { dto: {
            runId,
            expectedTerminalSeq: checkpoint.terminalSeq,
            chatRef,
            stableChatId,
            additionalRounds,
            hostPresentation: true,
        } });
        attachRunBridges(handle, {
            chatRef,
            stableChatId,
            generationType: handle.generationType,
            options: { presentation: presentation.liveEnabled ? 'foreground' : 'background' },
        }, presentation);
        return handle;
    }

    async function startRunFromLegacyGenerate(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent startRunFromLegacyGenerate input must be an object');
        }

        const generationType = normalizeGenerationType(input.generationType);
        const agentOptions = normalizeAgentRunOptions(input.options, input.presentation);
        const runProfile = await ensureRunProfileModelTargetConnection(input.profileId, safeInvoke);
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

        return startRunWithPromptSnapshotInternal({
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            generationType,
            profileId: input.profileId,
            persistBaseStateId: input.persistBaseStateId,
            promptSnapshot: snapshot.promptSnapshot,
            frozenRunInputSnapshot: snapshot.frozenRunInputSnapshot,
            generationIntent: mergePlainObject(snapshot.generationIntent, input.generationIntent),
            options: agentOptions,
        }, {
            ensureModelTargetConnection: false,
            runProfile,
        });
    }

    async function listTools() {
        return safeInvoke('list_agent_tools');
    }

    async function copyChatPersistentStates(input) {
        return safeInvoke('copy_agent_chat_persistent_states', { dto: input });
    }

    async function settleChatPresentation({ runId }) {
        const bridge = commitBridges.get(runId);
        if (bridge) await settleHostCommitBridge(bridge);
    }

    return {
        startRunWithPromptSnapshot,
        startRunFromLegacyGenerate,
        readCheckpoint,
        resume,
        cancel: runtime.cancel,
        submitGuidance: runtime.submitGuidance,
        readEvents: runtime.readEvents,
        readWorkspaceFile: runtime.readWorkspaceFile,
        readTaskDetail: runtime.readTaskDetail,
        readModelTurn: runtime.readModelTurn,
        pruneChatPersistentStates: runtime.pruneChatPersistentStates,
        copyChatPersistentStates,
        retention: runtime.retention,
        subscribe: runtime.subscribe,
        subscribeLiveProjection,
        settleChatPresentation,
        profiles,
        tools: {
            list: listTools,
        },
        promptAssembly,
        approveToolCall() {
            throw new Error('approveToolCall is not implemented');
        },
        listRuns: runtime.listRuns,
    };
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

async function normalizePromptSnapshotRunInput(input, { safeInvoke, ensureModelTargetConnection = true, runProfile = null }) {
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

    const options = normalizeAgentRunOptions(input.options, input.presentation);
    let resolvedRunProfile = runProfile;
    if (ensureModelTargetConnection) {
        resolvedRunProfile = await ensureRunProfileModelTargetConnection(input.profileId ?? input.profile_id, safeInvoke);
    }
    const skillScopeRefs = await resolveSkillScopeRefsForRun(input, safeInvoke, resolvedRunProfile);

    return {
        ...input,
        chatRef,
        stableChatId,
        skillScopeRefs,
        persistBaseStateId: normalizeOptionalString(input.persistBaseStateId),
        options: { ...normalizeAgentRunOptions(
            options,
            options.presentation ?? resolvedRunProfile?.run?.presentation,
        ), hostPresentation: true },
    };
}

async function resolveSkillScopeRefsForRun(input, safeInvoke, runProfile = null) {
    const refs = normalizeSkillScopeRefs(input.skillScopeRefs ?? input.skill_scope_refs);
    if (refs.preset) {
        return refs;
    }

    const profile = runProfile || await loadRunProfile(input.profileId ?? input.profile_id, safeInvoke);
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

async function ensureRunProfileModelTargetConnection(profileId, safeInvoke) {
    const profile = await loadRunProfile(profileId, safeInvoke);
    await ensureModelTargetLlmConnectionForProfile(profile);
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

function normalizeOptionalString(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const text = String(value).trim();
    return text || undefined;
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
