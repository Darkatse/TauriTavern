// @ts-check

import { emitAgentProfilesChanged } from '../../../scripts/tauritavern/agent/agent-profile-events.js';

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
export function createAgentProfilesApi({ safeInvoke }) {
    async function listProfiles() {
        return safeInvoke('list_agent_profiles');
    }

    async function loadProfile(input) {
        const profileId = requireProfileId(input?.profileId ?? input?.profile_id ?? input);
        return safeInvoke('load_agent_profile', { dto: { profileId } });
    }

    async function resolveSystemPrompt(input = {}) {
        const profileId = normalizeOptionalString(input?.profileId ?? input?.profile_id ?? input);
        return safeInvoke('resolve_agent_system_prompt', {
            dto: {
                ...(profileId ? { profileId } : {}),
            },
        });
    }

    async function retargetPresetRefs(input) {
        if (!isPlainObject(input)) {
            throw new Error('agent.profile_preset_retarget_input_invalid: retarget input must be an object');
        }
        const from = normalizeRetargetPresetRef(input.from, 'from');
        const to = normalizeRetargetPresetRef(input.to, 'to');
        const result = await safeInvoke('retarget_agent_profile_preset_refs', {
            dto: { from, to },
        });
        emitAgentProfilesChanged();
        return result;
    }

    async function saveProfile(input) {
        const profile = input?.profile ?? input;
        if (!isPlainObject(profile)) {
            throw new Error('agent.profile_required: profile must be an object');
        }
        const result = await safeInvoke('save_agent_profile', { dto: { profile } });
        emitAgentProfilesChanged();
        return result;
    }

    async function deleteProfile(input) {
        const profileId = requireProfileId(input?.profileId ?? input?.profile_id ?? input);
        const result = await safeInvoke('delete_agent_profile', { dto: { profileId } });
        emitAgentProfilesChanged();
        return result;
    }

    async function repairProfileFile(input) {
        if (!isPlainObject(input)) {
            throw new Error('agent.profile_repair_input_invalid: repair input must be an object');
        }
        const profileId = requireProfileId(input.profileId ?? input.profile_id);
        const action = normalizeProfileFileRepairAction(input.action);
        const result = await safeInvoke('repair_agent_profile_file', {
            dto: { profileId, action },
        });
        emitAgentProfilesChanged();
        return result;
    }

    return {
        list: listProfiles,
        load: loadProfile,
        resolveSystemPrompt,
        retargetPresetRefs,
        save: saveProfile,
        delete: deleteProfile,
        repairFile: repairProfileFile,
    };
}

function normalizeRetargetPresetRef(value, label) {
    if (!isPlainObject(value)) {
        throw new Error(`agent.profile_preset_retarget_${label}_invalid: ${label} must be an object`);
    }
    const apiId = normalizeOptionalString(value.apiId ?? value.api_id);
    const name = normalizeOptionalString(value.name);
    if (!apiId || !name) {
        throw new Error(`agent.profile_preset_retarget_${label}_invalid: ${label} requires apiId and name`);
    }
    return { apiId, name };
}

function requireProfileId(value) {
    const profileId = String(value || '').trim();
    if (!profileId) {
        throw new Error('profileId is required');
    }
    return profileId;
}

function normalizeProfileFileRepairAction(value) {
    const action = String(value || '').trim();
    if (action !== 'delete' && action !== 'normalizeIdentity') {
        throw new Error('agent.profile_repair_action_invalid: repair action must be delete or normalizeIdentity');
    }
    return action;
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
