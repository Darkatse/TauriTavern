// @ts-check

import { buildPromptAssemblySnapshot } from './agent-prompt-assembly.js';
import { materializeCurrentPromptSnapshot } from './agent-prompt-snapshot.js';

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
export function createPromptAssemblyApi({ safeInvoke }) {
    async function prepare(input = {}) {
        const dto = normalizePreparePromptAssemblyInput(input);
        return safeInvoke('prepare_agent_prompt_assembly', { dto });
    }

    return {
        prepare,
        buildSnapshot: buildPromptAssemblySnapshot,
    };
}

export async function assemblePromptSnapshotForProfile({
    generationType,
    profileId,
    jsonSchema,
    promptSnapshotResult,
    promptAssembly,
}) {
    const frozenRunInputSnapshot = promptSnapshotResult?.frozenRunInputSnapshot;
    if (!frozenRunInputSnapshot) {
        throw new Error('agent.frozen_run_input_snapshot_required: Agent prompt assembly requires FrozenRunInputSnapshot');
    }

    const prepared = await promptAssembly.prepare({
        profileId,
        generationType,
        frozenRunInputSnapshot,
        jsonSchema,
    });
    if (prepared?.mode === 'currentPromptSnapshot') {
        return materializeCurrentPromptSnapshot(promptSnapshotResult);
    }
    if (prepared?.mode !== 'frontendPromptAssembly') {
        throw new Error('agent.prompt_assembly_mode_invalid: prepare_agent_prompt_assembly returned an unsupported mode');
    }

    const assembled = await promptAssembly.buildSnapshot(prepared.request);
    return {
        promptSnapshot: assembled.promptSnapshot,
        frozenRunInputSnapshot: assembled.frozenRunInputSnapshot ?? frozenRunInputSnapshot,
        generationIntent: {
            ...assembled.generationIntent,
            parentSource: promptSnapshotResult.generationIntent?.source,
            promptAssembly: prepared.assembly,
        },
    };
}

function normalizePreparePromptAssemblyInput(input) {
    if (!isPlainObject(input)) {
        throw new Error('agent.prompt_assembly_input_invalid: input must be an object');
    }

    const generationType = normalizeGenerationType(input.generationType ?? input.generation_type);
    const frozenRunInputSnapshot = input.frozenRunInputSnapshot ?? input.frozen_run_input_snapshot;
    if (!isPlainObject(frozenRunInputSnapshot)) {
        throw new Error('agent.frozen_run_input_snapshot_required: FrozenRunInputSnapshot must be an object');
    }
    const profileId = normalizeOptionalString(input.profileId ?? input.profile_id);

    return {
        ...(profileId ? { profileId } : {}),
        generationType,
        frozenRunInputSnapshot,
        jsonSchema: input.jsonSchema ?? input.json_schema ?? null,
    };
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function normalizeOptionalString(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const text = String(value).trim();
    return text || undefined;
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
