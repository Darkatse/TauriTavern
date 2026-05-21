import { translateAgentSystem as tr } from './i18n.js';

export function skillScopeKey(scope) {
    if (!scope || typeof scope !== 'object') {
        return '';
    }
    if (scope.kind === 'global') {
        return 'global';
    }
    if (scope.kind === 'preset') {
        return `preset:${scope.apiId}:${scope.name}`;
    }
    if (scope.kind === 'profile') {
        return `profile:${scope.profileId}`;
    }
    if (scope.kind === 'character') {
        return `character:${scope.characterId}`;
    }
    return '';
}

export function skillScopeLabel(scope) {
    if (!scope || typeof scope !== 'object') {
        return tr('none');
    }
    if (scope.kind === 'global') {
        return tr('skillScopeGlobal');
    }
    if (scope.kind === 'preset') {
        return `${tr('skillScopePreset')} / ${String(scope.name || '').trim()}`;
    }
    if (scope.kind === 'profile') {
        return `${tr('skillScopeProfile')} / ${String(scope.profileId || '').trim()}`;
    }
    if (scope.kind === 'character') {
        return `${tr('skillScopeCharacter')} / ${String(scope.characterId || '').trim()}`;
    }
    return tr('none');
}
