import { DEFAULT_PROFILE_ID } from '../constants.js';
import { translateAgentSystem as tr } from '../i18n.js';
import { skillScopeKey, skillScopeLabel } from '../skill-scope.js';
import {
    characterStemFromAvatarFileName,
    hasCharacterAvatarIdentity,
} from '../../../../../tauri/main/services/characters/character-identity.js';

export { skillScopeKey, skillScopeLabel };

export const SKILL_SCOPE_IDS = Object.freeze(['global', 'preset', 'profile', 'character']);

const SCOPE_ICONS = Object.freeze({
    global: 'fa-globe',
    preset: 'fa-sliders',
    profile: 'fa-id-card-clip',
    character: 'fa-address-card',
});

const SCOPE_LABEL_KEYS = Object.freeze({
    global: 'skillScopeGlobal',
    preset: 'skillScopePreset',
    profile: 'skillScopeProfile',
    character: 'skillScopeCharacter',
});

function requireSillyTavernContext() {
    const context = window.SillyTavern?.getContext?.();
    if (!context) {
        throw new Error(tr('sillyTavernContextUnavailable'));
    }
    return context;
}

function normalizePresetApiId(apiId) {
    const value = String(apiId || '').trim();
    return value === 'koboldhorde' ? 'kobold' : value;
}

function currentPresetScope(context) {
    const apiId = normalizePresetApiId(context.mainApi);
    const presetManager = apiId ? context.getPresetManager?.(apiId) : null;
    const selectedValue = String(presetManager?.getSelectedPreset?.() || '').trim();
    const name = String(presetManager?.getSelectedPresetName?.() || '').trim();
    if (!apiId || !presetManager || !name || selectedValue === 'gui') {
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

function profileScope(profileId, profiles) {
    const id = String(profileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
    const profile = profiles.find((item) => item.id === id) || profiles[0] || null;
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

function characterScope(context) {
    const characterId = context.characterId;
    const character = context.characters?.[characterId];
    const avatar = character?.avatar;
    const resolvedId = hasCharacterAvatarIdentity(avatar)
        ? characterStemFromAvatarFileName(avatar, 'avatar', { required: true })
        : '';
    if (!character || !resolvedId) {
        return {
            available: false,
            unavailableKey: 'scopeUnavailableCharacter',
            subtitle: tr('none'),
            scope: null,
        };
    }

    return {
        available: true,
        subtitle: `${character.name || resolvedId} (${resolvedId})`,
        scope: { kind: 'character', characterId: resolvedId },
    };
}

export function buildSkillScopeSections({ selectedProfileId, profiles }) {
    const context = requireSillyTavernContext();
    const preset = currentPresetScope(context);
    const profile = profileScope(selectedProfileId, Array.isArray(profiles) ? profiles : []);
    const character = characterScope(context);

    return [
        {
            id: 'global',
            icon: SCOPE_ICONS.global,
            labelKey: SCOPE_LABEL_KEYS.global,
            available: true,
            subtitle: tr('skillScopeGlobalSubtitle'),
            scope: { kind: 'global' },
        },
        {
            id: 'preset',
            icon: SCOPE_ICONS.preset,
            labelKey: SCOPE_LABEL_KEYS.preset,
            ...preset,
        },
        {
            id: 'profile',
            icon: SCOPE_ICONS.profile,
            labelKey: SCOPE_LABEL_KEYS.profile,
            ...profile,
        },
        {
            id: 'character',
            icon: SCOPE_ICONS.character,
            labelKey: SCOPE_LABEL_KEYS.character,
            ...character,
        },
    ];
}
