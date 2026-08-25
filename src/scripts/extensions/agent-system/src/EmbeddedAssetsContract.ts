import { translateAgentSystem as tr } from './i18n.js';
import { skillScopeKey, skillScopeLabel } from './skill-scope.js';

export type EmbeddedAssetTargetInput =
    | { kind: 'preset'; apiId?: string; name?: string }
    | { kind: 'character' };

export type EmbeddedAssetTargetSummary = {
    kind: 'preset' | 'character';
    name: string;
    subtitle?: string;
};

export type EmbeddedProfileItem = {
    profile: {
        id: string;
        displayName?: string;
    };
};

export type EmbeddedSkillItem = {
    bundleFormat: string;
    skillName: string;
    sourceScope: TauriTavernSkillScope;
    sourceScopeLabel: string;
    fileName: string;
    contentBase64: string;
    sha256: string;
};

export type EmbeddedAssetsRead = {
    target: EmbeddedAssetTargetSummary;
    profiles: EmbeddedProfileItem[];
    skills: EmbeddedSkillItem[];
};

export type SkillOption = TauriTavernSkillIndexEntry & {
    key: string;
    scopeLabel: string;
};

export type EmbeddedAssetsInitial = {
    targetInfo: EmbeddedAssetTargetSummary;
    profiles: TauriTavernAgentProfileSummary[];
    skills: SkillOption[];
    embeddedProfiles: EmbeddedProfileItem[];
    embeddedSkills: EmbeddedSkillItem[];
};

export type EmbeddedAssetsActions = {
    // Resolves the embedded profile id so the panel can toast it.
    embedProfile: (profileId: string) => Promise<string>;
    embedSkill: (skill: SkillOption) => Promise<void>;
    removeProfile: (profileId: string) => Promise<void>;
    removeSkill: (skillName: string) => Promise<void>;
    // Synchronous re-read of the persisted embedded facts after a mutation.
    readEmbedded: () => EmbeddedAssetsRead;
    toastSuccess: (message: string) => void;
    // console + toastr; returns the message for inline display.
    reportError: (error: unknown) => string;
};

export function skillSelectionKey(skill: { scope?: TauriTavernSkillScope | null; name?: string | null }): string {
    const scopeKey = skillScopeKey(skill?.scope);
    const name = String(skill?.name || '').trim();
    if (!scopeKey || !name) {
        throw new Error(tr('skillScopeNotFound', { id: name || scopeKey || '' }));
    }
    return JSON.stringify({ scopeKey, name });
}

export function buildSkillOptions(skills: TauriTavernSkillIndexEntry[]): SkillOption[] {
    if (!Array.isArray(skills)) {
        throw new Error(tr('skillListMustBeArray'));
    }

    return skills
        .map((skill) => ({
            ...skill,
            key: skillSelectionKey(skill),
            scopeLabel: skillScopeLabel(skill.scope),
        }))
        .sort((left, right) => {
            const leftName = String(left.displayName || left.name || '');
            const rightName = String(right.displayName || right.name || '');
            return leftName.localeCompare(rightName, undefined, { sensitivity: 'base' })
                || left.scopeLabel.localeCompare(right.scopeLabel, undefined, { sensitivity: 'base' });
        });
}

export function profileDisplayName(item: EmbeddedProfileItem): string {
    return item.profile.displayName || item.profile.id;
}

export function skillOptionLabel(skill: SkillOption): string {
    return `${skill.displayName || skill.name} (${skill.scopeLabel})`;
}

export function embeddedSkillSubtitle(item: EmbeddedSkillItem): string {
    const sourceScopeLabel = String(item.sourceScopeLabel || '').trim();
    const fileName = String(item.fileName || '').trim();
    return sourceScopeLabel ? `${sourceScopeLabel} - ${fileName}` : fileName;
}
