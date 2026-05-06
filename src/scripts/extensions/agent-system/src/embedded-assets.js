import { DEFAULT_PROFILE_ID } from './constants.js';
import { clone, requireSkillApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';

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
    const presetManager = context.getPresetManager?.(apiId);
    if (!presetManager) {
        throw new Error(tr('presetManagerUnavailable'));
    }

    const selectedValue = String(presetManager.getSelectedPreset?.() || '').trim();
    if (selectedValue === 'gui') {
        throw new Error(tr('presetMustBeSaved'));
    }

    const name = String(presetManager.getSelectedPresetName?.() || '').trim();
    if (!name) {
        throw new Error(tr('presetSelectionRequired'));
    }
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
    return payload;
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
    const normalized = clone(requirePlainObject(profile, 'profile'));
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

function requireCharacterJsonDataField() {
    const field = document.getElementById('character_json_data');
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
    const field = requireCharacterJsonDataField();
    const current = clone(target.character?.data?.extensions?.tauritavern || {});
    const tauritavern = {
        ...current,
        ...patch,
    };
    const jsonData = buildCharacterJsonData(target.character, tauritavern);
    const serializedJsonData = JSON.stringify(jsonData);

    const response = await fetch('/api/characters/merge-attributes', {
        method: 'POST',
        headers: target.context.getRequestHeaders(),
        body: JSON.stringify({
            avatar: target.character.avatar,
            data: {
                extensions: {
                    tauritavern,
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
    target.character.data.extensions.tauritavern = tauritavern;
    target.character.json_data = serializedJsonData;
    field.value = serializedJsonData;
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

export async function embedSkill(targetInput, skillName) {
    const target = resolveTarget(targetInput);
    const next = upsertSkill(readSkillPackage(target), await buildEmbeddedSkillItem(skillName));
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

export async function buildEmbeddedSkillItem(skillName) {
    const payload = await requireSkillApi().exportSkill({ name: skillName });
    return {
        bundleFormat: SKILL_ARCHIVE_BUNDLE_FORMAT,
        skillName,
        fileName: payload.fileName,
        contentBase64: payload.contentBase64,
        sha256: payload.sha256,
    };
}
