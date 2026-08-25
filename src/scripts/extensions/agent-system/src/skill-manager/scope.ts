import { DEFAULT_PROFILE_ID } from '../constants.js';
import {
    characterStemFromAvatarFileName,
    hasCharacterAvatarIdentity,
} from '../../../../../tauri/main/services/characters/character-identity.js';
import type {
    SkillHostCharacter,
    SkillHostContext,
    SkillManagerTr,
    SkillScopeSection,
} from './SkillManagerContract';

const SCOPE_META = {
    global: { icon: 'fa-globe', labelKey: 'skillScopeGlobal' },
    preset: { icon: 'fa-sliders', labelKey: 'skillScopePreset' },
    profile: { icon: 'fa-id-card-clip', labelKey: 'skillScopeProfile' },
    character: { icon: 'fa-address-card', labelKey: 'skillScopeCharacter' },
} as const;

function normalizePresetApiId(value: string | null | undefined): string {
    const apiId = String(value || '').trim();
    return apiId === 'koboldhorde' ? 'kobold' : apiId;
}

function presetSection(context: SkillHostContext, tr: SkillManagerTr): Omit<SkillScopeSection, 'id' | 'icon' | 'labelKey'> {
    const apiId = normalizePresetApiId(context.mainApi);
    const manager = apiId ? context.getPresetManager?.(apiId) : null;
    const selectedValue = String(manager?.getSelectedPreset?.() || '').trim();
    const name = String(manager?.getSelectedPresetName?.() || '').trim();
    if (!apiId || !manager || !name || selectedValue === 'gui') {
        return {
            available: false,
            unavailableKey: 'scopeUnavailablePreset',
            subtitle: tr('none'),
            scope: null,
        };
    }
    return {
        available: true,
        subtitle: `${apiId} / ${name}`,
        scope: { kind: 'preset', apiId, name },
    };
}

function profileSection(
    selectedProfileId: string,
    profiles: readonly TauriTavernAgentProfileSummary[],
    tr: SkillManagerTr,
): Omit<SkillScopeSection, 'id' | 'icon' | 'labelKey'> {
    const id = selectedProfileId.trim() || DEFAULT_PROFILE_ID;
    const profile = profiles.find((item) => item.id === id) ?? profiles[0];
    if (!profile) {
        return {
            available: false,
            unavailableKey: 'scopeUnavailableProfile',
            subtitle: tr('none'),
            scope: null,
        };
    }
    return {
        available: true,
        subtitle: profile.displayName ? `${profile.displayName} (${profile.id})` : profile.id,
        scope: { kind: 'profile', profileId: profile.id },
    };
}

function currentCharacter(context: SkillHostContext): SkillHostCharacter | undefined {
    const id = context.characterId;
    if (id === null || id === undefined || !context.characters) {
        return undefined;
    }
    return Array.isArray(context.characters)
        ? context.characters[Number(id)]
        : context.characters[String(id)];
}

function characterSection(context: SkillHostContext, tr: SkillManagerTr): Omit<SkillScopeSection, 'id' | 'icon' | 'labelKey'> {
    const character = currentCharacter(context);
    const avatar = character?.avatar;
    const characterId = hasCharacterAvatarIdentity(avatar)
        ? characterStemFromAvatarFileName(avatar, 'avatar', { required: true })
        : '';
    if (!character || !characterId) {
        return {
            available: false,
            unavailableKey: 'scopeUnavailableCharacter',
            subtitle: tr('none'),
            scope: null,
        };
    }
    const name = String(character.name || characterId);
    return {
        available: true,
        subtitle: `${name} (${characterId})`,
        scope: { kind: 'character', characterId },
    };
}

export function buildSkillScopeSections(options: {
    context: SkillHostContext;
    selectedProfileId: string;
    profiles: readonly TauriTavernAgentProfileSummary[];
    tr: SkillManagerTr;
}): SkillScopeSection[] {
    const { context, selectedProfileId, profiles, tr } = options;
    return [
        {
            id: 'global',
            ...SCOPE_META.global,
            available: true,
            subtitle: tr('skillScopeGlobalSubtitle'),
            scope: { kind: 'global' },
        },
        { id: 'preset', ...SCOPE_META.preset, ...presetSection(context, tr) },
        { id: 'profile', ...SCOPE_META.profile, ...profileSection(selectedProfileId, profiles, tr) },
        { id: 'character', ...SCOPE_META.character, ...characterSection(context, tr) },
    ];
}
