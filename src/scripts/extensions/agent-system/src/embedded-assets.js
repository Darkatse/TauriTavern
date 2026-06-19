import { DEFAULT_PROFILE_ID } from './constants.js';
import { clone, requireSkillApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { skillScopeLabel } from './skill-scope.js';
import {
    sanitizePortableAgentProfile,
    sanitizePortableAgentProfilePackage,
} from '../../../tauritavern/agent/agent-profile-portable.js';
import {
    assertCharacterAvatarFileName,
    characterStemFromAvatarFileName,
} from '../../../../tauri/main/services/characters/character-identity.js';

const EMBEDDED_PROFILES_VERSION = 1;
const EMBEDDED_SKILLS_VERSION = 1;
const SKILL_ARCHIVE_BUNDLE_FORMAT = 'ttskill-archive-base64-v1';

const TARGET_KIND = Object.freeze({
    PRESET: 'preset',
    CHARACTER: 'character',
});

const PRESET_API_LABELS = Object.freeze({
    kobold: 'KoboldAI',
    novel: 'NovelAI',
    openai: 'Chat Completion',
    textgenerationwebui: 'Text Completion',
});

function requirePlainObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value;
}

function requireSillyTavernContext() {
    const context = window.SillyTavern?.getContext?.();
    if (!context) {
        throw new Error(tr('sillyTavernContextUnavailable'));
    }
    return context;
}

function requirePresetTarget(target) {
    const context = requireSillyTavernContext();
    const apiId = String(target?.apiId || '').trim();
    const name = String(target?.name || '').trim();
    const presetManager = context.getPresetManager?.(apiId);
    if (!presetManager) {
        throw new Error(tr('presetManagerUnavailable'));
    }

    if (name) {
        return requirePresetByName({ apiId, name, presetManager });
    }

    const selectedValue = String(presetManager.getSelectedPreset?.() || '').trim();
    const selectedName = String(presetManager.getSelectedPresetName?.() || '').trim();
    if (selectedValue === 'gui') {
        throw new Error(tr('presetMustBeSaved'));
    }
    if (!selectedName) {
        throw new Error(tr('presetSelectionRequired'));
    }
    return requirePresetByName({ apiId, name: selectedName, presetManager });
}

function requirePresetByName({ apiId, name, presetManager }) {
    if (typeof presetManager.getCompletionPresetByName !== 'function') {
        throw new Error(tr('presetManagerUnavailable'));
    }
    if (!presetManager.getCompletionPresetByName(name)) {
        throw new Error(tr('presetSelectionRequired'));
    }

    return {
        kind: TARGET_KIND.PRESET,
        apiId,
        name,
        presetManager,
    };
}

function requireCharacterTarget() {
    const context = requireSillyTavernContext();
    const characterId = context.characterId;
    const character = context.characters?.[characterId];
    if (!character) {
        throw new Error(tr('characterSelectionRequired'));
    }
    return {
        kind: TARGET_KIND.CHARACTER,
        context,
        characterId,
        character,
    };
}

function characterIdFromAvatar(avatar) {
    return characterStemFromAvatarFileName(avatar, 'avatar', { required: true });
}

function characterAvatarFileName(character) {
    return assertCharacterAvatarFileName(character?.avatar, 'avatar', { required: true });
}

function requireCharacterTargetByScope(scope) {
    const context = requireSillyTavernContext();
    const characterId = String(scope?.characterId ?? '');
    if (!characterId) {
        throw new Error(tr('skillScopeNotFound', { id: '' }));
    }
    const characters = Array.isArray(context.characters)
        ? context.characters
        : Object.values(context.characters || {});
    const character = characters.find((item) => characterIdFromAvatar(item?.avatar) === characterId);
    if (!character) {
        throw new Error(tr('characterSelectionRequired'));
    }
    return {
        kind: TARGET_KIND.CHARACTER,
        context,
        characterId,
        character,
    };
}

function resolveScopeTarget(scope) {
    const kind = String(scope?.kind || '').trim();
    if (kind === TARGET_KIND.PRESET) {
        return requirePresetTarget(scope);
    }
    if (kind === TARGET_KIND.CHARACTER) {
        return requireCharacterTargetByScope(scope);
    }
    throw new Error(tr('embeddedAssetTargetInvalid'));
}

function resolveTarget(target) {
    const kind = String(target?.kind || '').trim();
    if (kind === TARGET_KIND.PRESET) {
        return requirePresetTarget(target);
    }
    if (kind === TARGET_KIND.CHARACTER) {
        return requireCharacterTarget();
    }
    throw new Error(tr('embeddedAssetTargetInvalid'));
}

function targetSummary(target) {
    if (target.kind === TARGET_KIND.PRESET) {
        return {
            kind: target.kind,
            apiId: target.apiId,
            name: target.name,
            subtitle: PRESET_API_LABELS[target.apiId] || target.apiId || tr('targetPreset'),
        };
    }

    return {
        kind: target.kind,
        characterId: target.characterId,
        name: String(target.character.name || '').trim() || target.character.avatar,
        subtitle: target.character.avatar,
    };
}

function profilePackage(existing) {
    if (existing === null || existing === undefined) {
        return { version: EMBEDDED_PROFILES_VERSION, items: [] };
    }
    const payload = clone(requirePlainObject(existing, 'agentProfiles'));
    if (Number(payload.version) !== EMBEDDED_PROFILES_VERSION) {
        throw new Error(tr('embeddedProfileVersionUnsupported', { version: payload.version }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedProfileItemsInvalid'));
    }
    return sanitizePortableAgentProfilePackage(payload);
}

function skillPackage(existing) {
    if (existing === null || existing === undefined) {
        return { version: EMBEDDED_SKILLS_VERSION, items: [] };
    }
    const payload = clone(requirePlainObject(existing, 'skills'));
    if (Number(payload.version) !== EMBEDDED_SKILLS_VERSION) {
        throw new Error(tr('embeddedSkillVersionUnsupported', { version: payload.version }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedSkillItemsInvalid'));
    }
    return payload;
}

function upsertProfile(packageValue, profile) {
    const normalized = sanitizePortableAgentProfile(requirePlainObject(profile, 'profile'));
    const id = String(normalized.id || '').trim();
    if (!id) {
        throw new Error(tr('profileIdRequired'));
    }
    if (id === DEFAULT_PROFILE_ID) {
        throw new Error(tr('cannotEmbedBuiltinProfile'));
    }

    const item = { profile: normalized };
    const index = packageValue.items.findIndex((entry) => entry?.profile?.id === id);
    if (index >= 0) {
        packageValue.items[index] = item;
    } else {
        packageValue.items.push(item);
    }
    return packageValue;
}

function upsertSkill(packageValue, item) {
    const skillName = String(item.skillName || '').trim();
    if (!skillName) {
        throw new Error(tr('skillNameRequired'));
    }
    const index = packageValue.items.findIndex((entry) => entry?.skillName === skillName);
    if (index >= 0) {
        packageValue.items[index] = item;
    } else {
        packageValue.items.push(item);
    }
    return packageValue;
}

function requireSkillRef(value) {
    const skill = requirePlainObject(value, 'skill');
    const name = String(skill.name || '').trim();
    if (!name) {
        throw new Error(tr('skillNameRequired'));
    }
    const scope = clone(requirePlainObject(skill.scope, 'skill.scope'));
    return { name, scope };
}

function removeProfile(packageValue, profileId) {
    const id = String(profileId || '').trim();
    if (!id) {
        throw new Error(tr('profileIdRequired'));
    }
    packageValue.items = packageValue.items.filter((entry) => entry?.profile?.id !== id);
    return packageValue;
}

function removeSkill(packageValue, skillName) {
    const name = String(skillName || '').trim();
    if (!name) {
        throw new Error(tr('skillNameRequired'));
    }
    packageValue.items = packageValue.items.filter((entry) => entry?.skillName !== name);
    return packageValue;
}

function readProfilePackage(target) {
    if (target.kind === TARGET_KIND.PRESET) {
        return profilePackage(target.presetManager.readPresetExtensionField({
            name: target.name,
            path: 'tauritavern.agentProfiles',
        }));
    }
    return profilePackage(target.character?.data?.extensions?.tauritavern?.agentProfiles);
}

function readSkillPackage(target) {
    if (target.kind === TARGET_KIND.PRESET) {
        return skillPackage(target.presetManager.readPresetExtensionField({
            name: target.name,
            path: 'tauritavern.skills',
        }));
    }
    return skillPackage(target.character?.data?.extensions?.tauritavern?.skills);
}

function findCharacterJsonDataField() {
    if (typeof document === 'undefined') {
        return null;
    }
    const field = document.getElementById('character_json_data');
    if (field === null) {
        return null;
    }
    if (!(field instanceof HTMLInputElement)) {
        throw new Error(tr('characterJsonDataFieldUnavailable'));
    }
    return field;
}

function buildCharacterJsonData(character, tauritavern) {
    const jsonData = character.json_data ? JSON.parse(character.json_data) : {};
    jsonData.data = jsonData.data || {};
    jsonData.data.extensions = jsonData.data.extensions || {};
    jsonData.data.extensions.tauritavern = tauritavern;
    return jsonData;
}

async function writeCharacterTauriTavernPatch(target, patch) {
    const patchValue = clone(requirePlainObject(patch, 'tauritavern patch'));
    const current = clone(target.character?.data?.extensions?.tauritavern || {});
    const nextTauriTavern = {
        ...current,
        ...patchValue,
    };
    const jsonData = buildCharacterJsonData(target.character, nextTauriTavern);
    const serializedJsonData = JSON.stringify(jsonData);
    const avatar = characterAvatarFileName(target.character);

    const response = await fetch('/api/characters/merge-attributes', {
        method: 'POST',
        headers: target.context.getRequestHeaders(),
        body: JSON.stringify({
            avatar,
            data: {
                extensions: {
                    tauritavern: patchValue,
                },
            },
        }),
    });
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(details || response.statusText || `HTTP ${response.status}`);
    }

    target.character.data = target.character.data || {};
    target.character.data.extensions = target.character.data.extensions || {};
    target.character.data.extensions.tauritavern = nextTauriTavern;
    target.character.json_data = serializedJsonData;
    const field = findCharacterJsonDataField();
    if (field) {
        field.value = serializedJsonData;
    }
}

async function writeProfiles(target, packageValue) {
    if (target.kind === TARGET_KIND.PRESET) {
        await target.presetManager.writePresetExtensionField({
            name: target.name,
            path: 'tauritavern.agentProfiles',
            value: packageValue,
        });
        return;
    }
    await writeCharacterTauriTavernPatch(target, { agentProfiles: packageValue });
}

async function writeSkills(target, packageValue) {
    if (target.kind === TARGET_KIND.PRESET) {
        await target.presetManager.writePresetExtensionField({
            name: target.name,
            path: 'tauritavern.skills',
            value: packageValue,
        });
        return;
    }
    await writeCharacterTauriTavernPatch(target, { skills: packageValue });
}

export function readEmbeddedAssets(targetInput) {
    const target = resolveTarget(targetInput);
    return {
        target: targetSummary(target),
        profiles: readProfilePackage(target).items,
        skills: readSkillPackage(target).items,
    };
}

export async function embedProfile(targetInput, profile) {
    const target = resolveTarget(targetInput);
    const next = upsertProfile(readProfilePackage(target), profile);
    await writeProfiles(target, next);
}

export async function embedSkill(targetInput, skillRef) {
    const target = resolveTarget(targetInput);
    const next = upsertSkill(readSkillPackage(target), await buildEmbeddedSkillItem(skillRef));
    await writeSkills(target, next);
}

export async function embedSkillForScope(scope, skillName) {
    const target = resolveScopeTarget(scope);
    const next = upsertSkill(readSkillPackage(target), await buildEmbeddedSkillItem({
        scope,
        name: skillName,
    }));
    await writeSkills(target, next);
}

export async function removeEmbeddedProfile(targetInput, profileId) {
    const target = resolveTarget(targetInput);
    await writeProfiles(target, removeProfile(readProfilePackage(target), profileId));
}

export async function removeEmbeddedSkill(targetInput, skillName) {
    const target = resolveTarget(targetInput);
    await writeSkills(target, removeSkill(readSkillPackage(target), skillName));
}

export async function removeEmbeddedSkillForScope(scope, skillName) {
    const target = resolveScopeTarget(scope);
    await writeSkills(target, removeSkill(readSkillPackage(target), skillName));
}

export async function buildEmbeddedSkillItem(skillRef) {
    const skill = requireSkillRef(skillRef);
    const payload = await requireSkillApi().export({
        scope: skill.scope,
        name: skill.name,
    });
    return {
        bundleFormat: SKILL_ARCHIVE_BUNDLE_FORMAT,
        skillName: skill.name,
        sourceScope: skill.scope,
        sourceScopeLabel: skillScopeLabel(skill.scope),
        fileName: payload.fileName,
        contentBase64: payload.contentBase64,
        sha256: payload.sha256,
    };
}
