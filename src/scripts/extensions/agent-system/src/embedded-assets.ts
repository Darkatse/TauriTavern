import { DEFAULT_PROFILE_ID } from './constants';
import { clone, requireSillyTavernContext, requireSkillApi } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import { skillScopeLabel } from './skill-scope';
import type {
    EmbeddedAssetsRead,
    EmbeddedAssetTargetInput,
    EmbeddedAssetTargetSummary,
} from './EmbeddedAssetsContract';
import {
    EMBEDDED_SKILL_ARCHIVE_FORMAT,
    embeddedProfileSummary,
    embeddedSkillSummary,
    portableEmbeddedProfile,
    readEmbeddedProfilePackage,
    readEmbeddedSkillPackage,
    type EmbeddedProfilePackage,
    type EmbeddedSkillPackage,
    type StoredEmbeddedProfileItem,
    type StoredEmbeddedSkillItem,
} from './embedded-asset-packages';
import {
    assertCharacterAvatarFileName,
    characterStemFromAvatarFileName,
} from '../../../../tauri/main/services/characters/character-identity.js';

const TARGET_KIND = Object.freeze({
    PRESET: 'preset',
    CHARACTER: 'character',
});

const PRESET_API_LABELS: Readonly<Record<string, string>> = Object.freeze({
    kobold: 'KoboldAI',
    novel: 'NovelAI',
    openai: 'Chat Completion',
    textgenerationwebui: 'Text Completion',
});

type PresetManager = {
    getSelectedPreset?: () => unknown;
    getSelectedPresetName?: () => unknown;
    getCompletionPresetByName?: (name: string) => unknown;
    readPresetExtensionField: (input: { name: string; path: string }) => unknown;
    writePresetExtensionField: (input: { name: string; path: string; value: unknown }) => unknown;
};

type EmbeddedCharacter = {
    avatar?: unknown;
    name?: unknown;
    json_data?: string;
    data?: {
        extensions?: {
            tauritavern?: Record<string, unknown>;
        };
    };
};

type EmbeddedAssetsHostContext = {
    characterId?: unknown;
    characters?: Record<string, EmbeddedCharacter> | EmbeddedCharacter[];
    getPresetManager?: (apiId: string) => PresetManager | null | undefined;
    getRequestHeaders: () => HeadersInit;
};

type PresetTarget = {
    kind: 'preset';
    apiId: string;
    name: string;
    presetManager: PresetManager;
};

type CharacterTarget = {
    kind: 'character';
    context: EmbeddedAssetsHostContext;
    characterId: string;
    character: EmbeddedCharacter;
};

type ResolvedTarget = PresetTarget | CharacterTarget;

function requirePlainObject(value: unknown, label: string): Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value as Record<string, unknown>;
}

function requirePresetTarget(target: { apiId?: string; name?: string }): PresetTarget {
    const context = requireSillyTavernContext() as EmbeddedAssetsHostContext;
    const apiId = target.apiId?.trim() ?? '';
    const name = target.name?.trim() ?? '';
    const presetManager = context.getPresetManager?.(apiId);
    if (!presetManager) {
        throw new Error(tr('presetManagerUnavailable'));
    }

    if (name) {
        return requirePresetByName({ apiId, name, presetManager });
    }

    const selectedValue = stringValue(presetManager.getSelectedPreset?.()).trim();
    const selectedName = stringValue(presetManager.getSelectedPresetName?.()).trim();
    if (selectedValue === 'gui') {
        throw new Error(tr('presetMustBeSaved'));
    }
    if (!selectedName) {
        throw new Error(tr('presetSelectionRequired'));
    }
    return requirePresetByName({ apiId, name: selectedName, presetManager });
}

function requirePresetByName(input: { apiId: string; name: string; presetManager: PresetManager }): PresetTarget {
    const { apiId, name, presetManager } = input;
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

function requireCharacterTarget(): CharacterTarget {
    const context = requireSillyTavernContext() as EmbeddedAssetsHostContext;
    const characterId = stringValue(context.characterId);
    const characters = context.characters as Record<string, EmbeddedCharacter> | undefined;
    const character = characters?.[characterId];
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

function characterIdFromAvatar(avatar: unknown): string {
    return characterStemFromAvatarFileName(avatar, 'avatar', { required: true });
}

function characterAvatarFileName(character: EmbeddedCharacter): string {
    return assertCharacterAvatarFileName(character?.avatar, 'avatar', { required: true });
}

function requireCharacterTargetByScope(scope: Extract<TauriTavernSkillScope, { kind: 'character' }>): CharacterTarget {
    const context = requireSillyTavernContext() as EmbeddedAssetsHostContext;
    const characterId = scope.characterId;
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

function resolveScopeTarget(scope: TauriTavernSkillScope): ResolvedTarget {
    if (scope.kind === TARGET_KIND.PRESET) {
        return requirePresetTarget(scope);
    }
    if (scope.kind === TARGET_KIND.CHARACTER) {
        return requireCharacterTargetByScope(scope);
    }
    throw new Error(tr('embeddedAssetTargetInvalid'));
}

function resolveTarget(target: EmbeddedAssetTargetInput): ResolvedTarget {
    if (target.kind === TARGET_KIND.PRESET) {
        return requirePresetTarget(target);
    }
    if (target.kind === TARGET_KIND.CHARACTER) {
        return requireCharacterTarget();
    }
    throw new Error(tr('embeddedAssetTargetInvalid'));
}

function targetSummary(target: ResolvedTarget): EmbeddedAssetTargetSummary {
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
        name: stringValue(target.character.name).trim() || characterAvatarFileName(target.character),
        subtitle: characterAvatarFileName(target.character),
    };
}

function upsertProfile(packageValue: EmbeddedProfilePackage, profile: unknown): EmbeddedProfilePackage {
    const normalized = portableEmbeddedProfile(profile);
    const id = normalized.id;
    if (id === DEFAULT_PROFILE_ID) {
        throw new Error(tr('cannotEmbedBuiltinProfile'));
    }

    const item: StoredEmbeddedProfileItem = { profile: normalized };
    const index = packageValue.items.findIndex((entry) => entry?.profile?.id === id);
    if (index >= 0) {
        packageValue.items[index] = item;
    } else {
        packageValue.items.push(item);
    }
    return packageValue;
}

function upsertSkill(packageValue: EmbeddedSkillPackage, item: StoredEmbeddedSkillItem): EmbeddedSkillPackage {
    const skillName = item.skillName.trim();
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

function removeProfile(packageValue: EmbeddedProfilePackage, profileId: string): EmbeddedProfilePackage {
    const id = profileId.trim();
    if (!id) {
        throw new Error(tr('profileIdRequired'));
    }
    packageValue.items = packageValue.items.filter((entry) => entry?.profile?.id !== id);
    return packageValue;
}

function removeSkill(packageValue: EmbeddedSkillPackage, skillName: string): EmbeddedSkillPackage {
    const name = skillName.trim();
    if (!name) {
        throw new Error(tr('skillNameRequired'));
    }
    packageValue.items = packageValue.items.filter((entry) => entry?.skillName !== name);
    return packageValue;
}

function readProfilePackage(target: ResolvedTarget): EmbeddedProfilePackage {
    if (target.kind === TARGET_KIND.PRESET) {
        return readEmbeddedProfilePackage(target.presetManager.readPresetExtensionField({
            name: target.name,
            path: 'tauritavern.agentProfiles',
        }));
    }
    return readEmbeddedProfilePackage(target.character?.data?.extensions?.tauritavern?.agentProfiles);
}

function readSkillPackage(target: ResolvedTarget): EmbeddedSkillPackage {
    if (target.kind === TARGET_KIND.PRESET) {
        return readEmbeddedSkillPackage(target.presetManager.readPresetExtensionField({
            name: target.name,
            path: 'tauritavern.skills',
        }));
    }
    return readEmbeddedSkillPackage(target.character?.data?.extensions?.tauritavern?.skills);
}

function findCharacterJsonDataField(): HTMLInputElement | null {
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

function buildCharacterJsonData(
    character: EmbeddedCharacter,
    tauritavern: Record<string, unknown>,
): Record<string, unknown> {
    const parsed: unknown = character.json_data ? JSON.parse(character.json_data) : {};
    const root = requirePlainObject(parsed, 'character json_data');
    const data = root.data == null ? {} : requirePlainObject(root.data, 'character json_data.data');
    const extensions = data.extensions == null
        ? {}
        : requirePlainObject(data.extensions, 'character json_data.data.extensions');
    return {
        ...root,
        data: {
            ...data,
            extensions: { ...extensions, tauritavern },
        },
    };
}

async function writeCharacterTauriTavernPatch(target: CharacterTarget, patch: Record<string, unknown>): Promise<void> {
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

async function writeProfiles(target: ResolvedTarget, packageValue: EmbeddedProfilePackage): Promise<void> {
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

async function writeSkills(target: ResolvedTarget, packageValue: EmbeddedSkillPackage): Promise<void> {
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

export function readEmbeddedAssets(targetInput: EmbeddedAssetTargetInput): EmbeddedAssetsRead {
    const target = resolveTarget(targetInput);
    return {
        target: targetSummary(target),
        profiles: readProfilePackage(target).items.map(embeddedProfileSummary),
        skills: readSkillPackage(target).items.map(embeddedSkillSummary),
    };
}

export async function embedProfile(targetInput: EmbeddedAssetTargetInput, profile: unknown): Promise<void> {
    const target = resolveTarget(targetInput);
    const next = upsertProfile(readProfilePackage(target), profile);
    await writeProfiles(target, next);
}

export async function embedSkill(
    targetInput: EmbeddedAssetTargetInput,
    skillRef: { scope: TauriTavernSkillScope; name: string },
): Promise<void> {
    const target = resolveTarget(targetInput);
    const next = upsertSkill(readSkillPackage(target), await buildEmbeddedSkillItem(skillRef));
    await writeSkills(target, next);
}

export async function embedSkillForScope(scope: TauriTavernSkillScope, skillName: string): Promise<void> {
    const target = resolveScopeTarget(scope);
    const next = upsertSkill(readSkillPackage(target), await buildEmbeddedSkillItem({
        scope,
        name: skillName,
    }));
    await writeSkills(target, next);
}

export async function removeEmbeddedProfile(targetInput: EmbeddedAssetTargetInput, profileId: string): Promise<void> {
    const target = resolveTarget(targetInput);
    await writeProfiles(target, removeProfile(readProfilePackage(target), profileId));
}

export async function removeEmbeddedSkill(targetInput: EmbeddedAssetTargetInput, skillName: string): Promise<void> {
    const target = resolveTarget(targetInput);
    await writeSkills(target, removeSkill(readSkillPackage(target), skillName));
}

export async function removeEmbeddedSkillForScope(scope: TauriTavernSkillScope, skillName: string): Promise<void> {
    const target = resolveScopeTarget(scope);
    await writeSkills(target, removeSkill(readSkillPackage(target), skillName));
}

async function buildEmbeddedSkillItem(
    skillRef: { scope: TauriTavernSkillScope; name: string },
): Promise<StoredEmbeddedSkillItem> {
    const name = skillRef.name.trim();
    if (!name) throw new Error(tr('skillNameRequired'));
    const payload = await requireSkillApi().export({
        scope: skillRef.scope,
        name,
    });
    return {
        bundleFormat: EMBEDDED_SKILL_ARCHIVE_FORMAT,
        skillName: name,
        sourceScope: skillRef.scope,
        sourceScopeLabel: skillScopeLabel(skillRef.scope),
        fileName: payload.fileName,
        contentBase64: payload.contentBase64,
        sha256: payload.sha256,
    };
}

function stringValue(value: unknown): string {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? String(value)
        : '';
}
