import type {
    EmbeddedAssetsRead,
    EmbeddedAssetTargetInput,
    EmbeddedSkillItem,
} from './EmbeddedAssetsContract';

export function readEmbeddedAssets(target: EmbeddedAssetTargetInput): EmbeddedAssetsRead;
export function embedProfile(target: EmbeddedAssetTargetInput, profile: unknown): Promise<void>;
export function embedSkill(target: EmbeddedAssetTargetInput, skillRef: { scope: TauriTavernSkillScope; name: string }): Promise<void>;
export function embedSkillForScope(scope: TauriTavernSkillScope, skillName: string): Promise<void>;
export function removeEmbeddedProfile(target: EmbeddedAssetTargetInput, profileId: string): Promise<void>;
export function removeEmbeddedSkill(target: EmbeddedAssetTargetInput, skillName: string): Promise<void>;
export function removeEmbeddedSkillForScope(scope: TauriTavernSkillScope, skillName: string): Promise<void>;
export function buildEmbeddedSkillItem(skillRef: { scope: TauriTavernSkillScope; name: string }): Promise<EmbeddedSkillItem>;
