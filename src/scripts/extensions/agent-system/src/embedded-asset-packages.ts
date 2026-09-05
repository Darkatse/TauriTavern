import type { EmbeddedProfileItem, EmbeddedSkillItem } from './EmbeddedAssetsContract';
import { translateAgentSystem as tr } from './i18n';
import {
    sanitizePortableAgentProfile,
    sanitizePortableAgentProfilePackage,
} from '../../../tauritavern/agent/agent-profile-portable.js';

const EMBEDDED_PROFILES_VERSION = 1;
const EMBEDDED_SKILLS_VERSION = 1;
export const EMBEDDED_SKILL_ARCHIVE_FORMAT = 'ttskill-archive-base64-v1';

export type StoredEmbeddedProfile = Record<string, unknown> & { id: string; displayName: string };
export type StoredEmbeddedProfileItem = { profile: StoredEmbeddedProfile };
export type StoredEmbeddedSkillItem = {
    bundleFormat: string;
    skillName: string;
    sourceScope: TauriTavernSkillScope;
    sourceScopeLabel: string;
    fileName: string;
    contentBase64: string;
    sha256: string;
};
export type EmbeddedProfilePackage = { version: number; items: StoredEmbeddedProfileItem[] };
export type EmbeddedSkillPackage = { version: number; items: StoredEmbeddedSkillItem[] };

export function readEmbeddedProfilePackage(existing: unknown): EmbeddedProfilePackage {
    if (existing == null) return { version: EMBEDDED_PROFILES_VERSION, items: [] };
    const payload = sanitizePortableAgentProfilePackage(existing);
    return {
        version: payload.version,
        items: payload.items.map((item, index) => profileItem(item, `agentProfiles.items[${index}]`)),
    };
}

export function readEmbeddedSkillPackage(existing: unknown): EmbeddedSkillPackage {
    if (existing == null) return { version: EMBEDDED_SKILLS_VERSION, items: [] };
    const payload = plainObject(existing, 'skills');
    if (Number(payload.version) !== EMBEDDED_SKILLS_VERSION) {
        throw new Error(tr('embeddedSkillVersionUnsupported', { version: scalarText(payload.version) }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedSkillItemsInvalid'));
    }
    return {
        version: EMBEDDED_SKILLS_VERSION,
        items: payload.items.map((item, index) => skillItem(item, `skills.items[${index}]`)),
    };
}

export function portableEmbeddedProfile(profile: unknown): StoredEmbeddedProfile {
    return profileItem({
        profile: sanitizePortableAgentProfile(plainObject(profile, 'profile')),
    }, 'profile').profile;
}

export function embeddedProfileSummary(item: StoredEmbeddedProfileItem): EmbeddedProfileItem {
    return {
        profile: {
            id: item.profile.id,
            ...(item.profile.displayName ? { displayName: item.profile.displayName } : {}),
        },
    };
}

export function embeddedSkillSummary(item: StoredEmbeddedSkillItem): EmbeddedSkillItem {
    return {
        skillName: item.skillName,
        sourceScopeLabel: item.sourceScopeLabel,
        fileName: item.fileName,
    };
}

function profileItem(value: unknown, label: string): StoredEmbeddedProfileItem {
    const item = plainObject(value, label);
    const profile = plainObject(item.profile, `${label}.profile`);
    const id = nonEmptyString(profile.id, `${label}.profile.id`);
    const displayName = profile.displayName == null
        ? ''
        : requireString(profile.displayName, `${label}.profile.displayName`);
    return { profile: { ...profile, id, displayName } };
}

function skillItem(value: unknown, label: string): StoredEmbeddedSkillItem {
    const item = plainObject(value, label);
    const bundleFormat = nonEmptyString(item.bundleFormat, `${label}.bundleFormat`);
    if (bundleFormat !== EMBEDDED_SKILL_ARCHIVE_FORMAT) {
        throw new Error(`Unsupported embedded Agent Skill bundle format: ${bundleFormat}`);
    }
    return {
        bundleFormat,
        skillName: nonEmptyString(item.skillName, `${label}.skillName`),
        sourceScope: skillScope(item.sourceScope, `${label}.sourceScope`),
        sourceScopeLabel: requireString(item.sourceScopeLabel, `${label}.sourceScopeLabel`),
        fileName: nonEmptyString(item.fileName, `${label}.fileName`),
        contentBase64: nonEmptyString(item.contentBase64, `${label}.contentBase64`),
        sha256: requireString(item.sha256, `${label}.sha256`).trim(),
    };
}

function skillScope(value: unknown, label: string): TauriTavernSkillScope {
    const scope = plainObject(value, label);
    const kind = nonEmptyString(scope.kind, `${label}.kind`);
    if (kind === 'global') return { kind };
    if (kind === 'preset') {
        return {
            kind,
            apiId: nonEmptyString(scope.apiId, `${label}.apiId`),
            name: nonEmptyString(scope.name, `${label}.name`),
        };
    }
    if (kind === 'profile') return { kind, profileId: nonEmptyString(scope.profileId, `${label}.profileId`) };
    if (kind === 'character') return { kind, characterId: nonEmptyString(scope.characterId, `${label}.characterId`) };
    throw new Error(`Unsupported Skill scope kind: ${kind}`);
}

function plainObject(value: unknown, label: string): Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value as Record<string, unknown>;
}

function requireString(value: unknown, label: string): string {
    if (typeof value !== 'string') throw new Error(`${label} must be a string`);
    return value;
}

function nonEmptyString(value: unknown, label: string): string {
    const text = requireString(value, label).trim();
    if (!text) throw new Error(`${label} is required`);
    return text;
}

function scalarText(value: unknown): string {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? String(value)
        : '';
}
