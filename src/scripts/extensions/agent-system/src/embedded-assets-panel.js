import { DEFAULT_PROFILE_ID } from './constants.js';
import { AGENT_TOGGLE_ICON } from './agent-icon.js';
import {
    embedProfile,
    embedSkill,
    readEmbeddedAssets,
    removeEmbeddedProfile,
    removeEmbeddedSkill,
} from './embedded-assets.js';
import { errorText, requireAgentApi, requireSkillApi } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { skillScopeKey, skillScopeLabel } from './skill-scope.js';

function skillSelectionKey(skill) {
    const scopeKey = skillScopeKey(skill?.scope);
    const name = String(skill?.name || '').trim();
    if (!scopeKey || !name) {
        throw new Error(tr('skillScopeNotFound', { id: name || scopeKey || '' }));
    }
    return JSON.stringify({ scopeKey, name });
}

function sortSkillOptions(skills) {
    return [...skills].sort((left, right) => {
        const leftName = String(left.displayName || left.name || '');
        const rightName = String(right.displayName || right.name || '');
        return leftName.localeCompare(rightName, undefined, { sensitivity: 'base' })
            || left.scopeLabel.localeCompare(right.scopeLabel, undefined, { sensitivity: 'base' });
    });
}

function buildSkillOptions(skills) {
    if (!Array.isArray(skills)) {
        throw new Error(tr('skillListMustBeArray'));
    }

    return sortSkillOptions(skills.map((skill) => ({
        ...skill,
        key: skillSelectionKey(skill),
        scopeLabel: skillScopeLabel(skill.scope),
    })));
}

export function createEmbeddedAssetsPanelRoot({ target, requestClose }) {
    return {
        data() {
            return {
                initialized: false,
                loading: false,
                saving: false,
                error: '',
                targetInfo: null,
                profiles: [],
                skills: [],
                embeddedProfiles: [],
                embeddedSkills: [],
                selectedProfileId: '',
                selectedSkillKey: '',
                agentIcon: AGENT_TOGGLE_ICON,
            };
        },
        computed: {
            embeddableProfiles() {
                return this.profiles.filter((profile) => profile.id !== DEFAULT_PROFILE_ID);
            },
            selectedProfileEmbedded() {
                return this.embeddedProfiles.some((item) => item?.profile?.id === this.selectedProfileId);
            },
            selectedSkillEmbedded() {
                const skill = this.selectedSkill;
                return Boolean(skill) && this.embeddedSkills.some((item) => item?.skillName === skill.name);
            },
            selectedSkill() {
                return this.skills.find((skill) => skill.key === this.selectedSkillKey) || null;
            },
            profileActionLabel() {
                return this.selectedProfileEmbedded ? tr('updateEmbeddedAsset') : tr('embedProfile');
            },
            skillActionLabel() {
                return this.selectedSkillEmbedded ? tr('updateEmbeddedAsset') : tr('embedSkill');
            },
            targetTypeLabel() {
                if (!this.targetInfo) {
                    return '';
                }
                return this.targetInfo.kind === 'preset' ? tr('targetPreset') : tr('targetCharacter');
            },
        },
        async mounted() {
            await this.refreshAll();
        },
        methods: {
            tr(key, params) {
                return tr(key, params);
            },
            requestClose,
            toast(message) {
                window.toastr?.success?.(message);
            },
            reportError(error) {
                const message = errorText(error);
                this.error = message;
                console.error('[AgentSystem]', error);
                window.toastr?.error?.(message);
            },
            async refreshAll() {
                this.loading = true;
                this.error = '';
                try {
                    const [profileResult, skills, embedded] = await Promise.all([
                        requireAgentApi().profiles.list(),
                        requireSkillApi().list({ scope: { kind: 'all' } }),
                        Promise.resolve(readEmbeddedAssets(target)),
                    ]);
                    this.profiles = Array.isArray(profileResult?.profiles) ? profileResult.profiles : [];
                    this.skills = buildSkillOptions(skills);
                    this.applyEmbeddedState(embedded);
                    this.syncSelections();
                    this.initialized = true;
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.loading = false;
                }
            },
            refreshEmbedded() {
                const embedded = readEmbeddedAssets(target);
                this.applyEmbeddedState(embedded);
                this.syncSelections();
            },
            applyEmbeddedState(embedded) {
                this.targetInfo = embedded.target;
                this.embeddedProfiles = Array.isArray(embedded.profiles) ? embedded.profiles : [];
                this.embeddedSkills = Array.isArray(embedded.skills) ? embedded.skills : [];
            },
            syncSelections() {
                if (!this.embeddableProfiles.some((profile) => profile.id === this.selectedProfileId)) {
                    this.selectedProfileId = this.embeddableProfiles[0]?.id || '';
                }
                if (!this.skills.some((skill) => skill.key === this.selectedSkillKey)) {
                    this.selectedSkillKey = this.skills[0]?.key || '';
                }
            },
            async runAssetAction(action) {
                this.saving = true;
                this.error = '';
                try {
                    await action();
                    this.refreshEmbedded();
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.saving = false;
                }
            },
            async embedSelectedProfile() {
                if (!this.selectedProfileId) {
                    throw new Error(tr('noEmbeddableProfiles'));
                }
                await this.runAssetAction(async () => {
                    const result = await requireAgentApi().profiles.load({ profileId: this.selectedProfileId });
                    const profile = result?.profile;
                    if (!profile) {
                        throw new Error(tr('agentProfileNotFound', { id: this.selectedProfileId }));
                    }
                    await embedProfile(target, profile);
                    this.toast(tr('embeddedProfile', { id: profile.id }));
                });
            },
            async embedSelectedSkill() {
                const skill = this.selectedSkill;
                if (!skill) {
                    throw new Error(tr('selectSkillFirst'));
                }
                await this.runAssetAction(async () => {
                    await embedSkill(target, skill);
                    this.toast(tr('embeddedSkill', { name: this.skillOptionLabel(skill) }));
                });
            },
            async removeProfileItem(item) {
                const profileId = item?.profile?.id;
                await this.runAssetAction(async () => {
                    await removeEmbeddedProfile(target, profileId);
                    this.toast(tr('removedEmbeddedProfile', { id: profileId }));
                });
            },
            async removeSkillItem(item) {
                const skillName = item?.skillName;
                await this.runAssetAction(async () => {
                    await removeEmbeddedSkill(target, skillName);
                    this.toast(tr('removedEmbeddedSkill', { name: skillName }));
                });
            },
            profileDisplayName(item) {
                return item?.profile?.displayName || item?.profile?.id || '';
            },
            skillDisplayName(skill) {
                return skill.displayName || skill.name;
            },
            skillOptionLabel(skill) {
                return `${this.skillDisplayName(skill)} (${skill.scopeLabel})`;
            },
            embeddedSkillSubtitle(item) {
                const sourceScopeLabel = String(item?.sourceScopeLabel || '').trim();
                const fileName = String(item?.fileName || '').trim();
                return sourceScopeLabel ? `${sourceScopeLabel} - ${fileName}` : fileName;
            },
        },
        template: `
            <div class="ttas-root ttas-embed-panel">
                <header class="ttas-embed-titlebar">
                    <div class="ttas-embed-title-icon" v-html="agentIcon"></div>
                    <div class="ttas-embed-title-copy">
                        <span>{{ targetTypeLabel || tr('agentAssets') }}</span>
                        <h3>{{ tr('agentAssets') }}</h3>
                        <p v-if="targetInfo">{{ targetInfo.name }}</p>
                    </div>
                    <button type="button" class="menu_button menu_button_icon ttas-embed-close" :aria-label="tr('close')" @click="requestClose">
                        <i class="fa-solid fa-xmark"></i>
                    </button>
                </header>

                <main class="ttas-embed-body">
                    <div v-if="loading && !initialized" class="ttas-embed-loading" role="status" aria-live="polite">
                        <i class="fa-solid fa-spinner fa-spin"></i>
                        <span>{{ tr('embedAssetPanelLoading') }}</span>
                    </div>

                    <template v-else>
                        <div v-if="targetInfo" class="ttas-embed-target">
                            <i class="fa-solid" :class="targetInfo.kind === 'preset' ? 'fa-sliders' : 'fa-id-card'"></i>
                            <div>
                                <span>{{ targetTypeLabel }}</span>
                                <strong>{{ targetInfo.name }}</strong>
                                <small v-if="targetInfo.subtitle">{{ targetInfo.subtitle }}</small>
                            </div>
                        </div>

                        <div v-if="error" class="ttas-embed-error" role="alert">
                            <i class="fa-solid fa-triangle-exclamation"></i>
                            <span>{{ error }}</span>
                        </div>

                        <section class="ttas-embed-card">
                            <div class="ttas-embed-section-title">
                                <i class="fa-solid fa-id-card-clip"></i>
                                <h4>{{ tr('profiles') }}</h4>
                            </div>
                            <div class="ttas-embed-action-row">
                                <label class="ttas-field">
                                    <span>{{ tr('selectProfile') }}</span>
                                    <select v-model="selectedProfileId" :disabled="saving || embeddableProfiles.length === 0">
                                        <option v-for="profile in embeddableProfiles" :key="profile.id" :value="profile.id">{{ profile.displayName || profile.id }}</option>
                                    </select>
                                </label>
                                <button type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="saving || !selectedProfileId" @click="embedSelectedProfile">
                                    <i class="fa-solid" :class="saving ? 'fa-spinner fa-spin' : 'fa-file-arrow-down'"></i>
                                    <span>{{ profileActionLabel }}</span>
                                </button>
                            </div>
                            <p v-if="embeddableProfiles.length === 0" class="ttas-embed-empty">{{ tr('noEmbeddableProfiles') }}</p>
                        </section>

                        <section class="ttas-embed-card">
                            <div class="ttas-embed-section-title">
                                <i class="fa-solid fa-book-bookmark"></i>
                                <h4>{{ tr('skills') }}</h4>
                            </div>
                            <div class="ttas-embed-action-row">
                                <label class="ttas-field">
                                    <span>{{ tr('selectSkill') }}</span>
                                    <select v-model="selectedSkillKey" :disabled="saving || skills.length === 0">
                                        <option v-for="skill in skills" :key="skill.key" :value="skill.key">{{ skillOptionLabel(skill) }}</option>
                                    </select>
                                </label>
                                <button type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="saving || !selectedSkill" @click="embedSelectedSkill">
                                    <i class="fa-solid" :class="saving ? 'fa-spinner fa-spin' : 'fa-file-zipper'"></i>
                                    <span>{{ skillActionLabel }}</span>
                                </button>
                            </div>
                            <p v-if="skills.length === 0" class="ttas-embed-empty">{{ tr('noSkillsInstalled') }}</p>
                        </section>

                        <section class="ttas-embed-card ttas-embed-current">
                            <div class="ttas-embed-section-title">
                                <i class="fa-solid fa-layer-group"></i>
                                <h4>{{ tr('embeddedAssets') }}</h4>
                            </div>

                            <div class="ttas-embedded-group">
                                <h5>{{ tr('embeddedProfiles') }}</h5>
                                <div v-if="embeddedProfiles.length > 0" class="ttas-embedded-list">
                                    <div v-for="item in embeddedProfiles" :key="item.profile.id" class="ttas-embedded-item">
                                        <i class="fa-solid fa-id-card-clip"></i>
                                        <div>
                                            <strong>{{ profileDisplayName(item) }}</strong>
                                            <span>{{ item.profile.id }}</span>
                                        </div>
                                        <button type="button" class="menu_button menu_button_icon ttas-danger-button" :title="tr('removeEmbeddedAsset')" :aria-label="tr('removeEmbeddedAsset')" :disabled="saving" @click="removeProfileItem(item)">
                                            <i class="fa-solid fa-xmark"></i>
                                        </button>
                                    </div>
                                </div>
                                <p v-else class="ttas-embed-empty">{{ tr('noEmbeddedProfiles') }}</p>
                            </div>

                            <div class="ttas-embedded-group">
                                <h5>{{ tr('embeddedSkills') }}</h5>
                                <div v-if="embeddedSkills.length > 0" class="ttas-embedded-list">
                                    <div v-for="item in embeddedSkills" :key="item.skillName" class="ttas-embedded-item">
                                        <i class="fa-solid fa-book-bookmark"></i>
                                        <div>
                                            <strong>{{ item.skillName }}</strong>
                                            <span>{{ embeddedSkillSubtitle(item) }}</span>
                                        </div>
                                        <button type="button" class="menu_button menu_button_icon ttas-danger-button" :title="tr('removeEmbeddedAsset')" :aria-label="tr('removeEmbeddedAsset')" :disabled="saving" @click="removeSkillItem(item)">
                                            <i class="fa-solid fa-xmark"></i>
                                        </button>
                                    </div>
                                </div>
                                <p v-else class="ttas-embed-empty">{{ tr('noEmbeddedSkills') }}</p>
                            </div>
                        </section>
                    </template>
                </main>
            </div>
        `,
    };
}
