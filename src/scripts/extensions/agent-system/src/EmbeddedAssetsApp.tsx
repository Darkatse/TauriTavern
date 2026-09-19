import { useEffect, useState } from 'react';

import { AGENT_TOGGLE_ICON } from './agent-icon';
import { DEFAULT_PROFILE_ID } from './constants';
import {
    type EmbeddedAssetsActions,
    type EmbeddedAssetsInitial,
    type EmbeddedAssetsRead,
    type EmbeddedProfileItem,
    type EmbeddedSkillItem,
    embeddedSkillSubtitle,
    profileDisplayName,
    skillOptionLabel,
    type SkillOption,
} from './EmbeddedAssetsContract';
import type { AgentSystemTr } from './i18n';

export type EmbeddedAssetsAppProps = {
    initialLoad: Promise<EmbeddedAssetsInitial>;
    actions: EmbeddedAssetsActions;
    tr: AgentSystemTr;
    onRequestClose: () => void;
};

type PanelState = {
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    targetInfo: EmbeddedAssetsInitial['targetInfo'] | null;
    profiles: TauriTavernAgentProfileSummary[];
    skills: SkillOption[];
    embeddedProfiles: EmbeddedProfileItem[];
    embeddedSkills: EmbeddedSkillItem[];
    selectedProfileId: string;
    selectedSkillKey: string;
};

const INITIAL_STATE: PanelState = {
    initialized: false,
    loading: true,
    saving: false,
    error: '',
    targetInfo: null,
    profiles: [],
    skills: [],
    embeddedProfiles: [],
    embeddedSkills: [],
    selectedProfileId: '',
    selectedSkillKey: '',
};

function embeddableProfilesOf(state: PanelState): TauriTavernAgentProfileSummary[] {
    return state.profiles.filter((profile) => profile.id !== DEFAULT_PROFILE_ID);
}

// Selections point at list entries; when the underlying lists change, fall
// back to the first available entry instead of leaving a dangling selection.
function withSyncedSelections(state: PanelState): PanelState {
    const embeddable = embeddableProfilesOf(state);
    const selectedProfileId = embeddable.some((profile) => profile.id === state.selectedProfileId)
        ? state.selectedProfileId
        : (embeddable[0]?.id ?? '');
    const selectedSkillKey = state.skills.some((skill) => skill.key === state.selectedSkillKey)
        ? state.selectedSkillKey
        : (state.skills[0]?.key ?? '');
    if (selectedProfileId === state.selectedProfileId && selectedSkillKey === state.selectedSkillKey) {
        return state;
    }
    return { ...state, selectedProfileId, selectedSkillKey };
}

export function EmbeddedAssetsApp({ initialLoad, actions, tr, onRequestClose }: EmbeddedAssetsAppProps) {
    const [state, setState] = useState<PanelState>(INITIAL_STATE);

    // The composition root starts the load once per dialog; this effect only
    // subscribes to that promise, so StrictMode cannot duplicate Host reads.
    useEffect(() => {
        let live = true;
        initialLoad.then(
            (data) => {
                if (!live) {
                    return;
                }
                setState((current) => withSyncedSelections({
                    ...current,
                    initialized: true,
                    loading: false,
                    error: '',
                    targetInfo: data.targetInfo,
                    profiles: data.profiles,
                    skills: data.skills,
                    embeddedProfiles: data.embeddedProfiles,
                    embeddedSkills: data.embeddedSkills,
                }));
            },
            (error: unknown) => {
                if (!live) {
                    return;
                }
                const message = actions.reportError(error);
                setState((current) => ({ ...current, loading: false, error: message }));
            },
        );
        return () => {
            live = false;
        };
    }, [initialLoad, actions]);

    function applyEmbedded(embedded: EmbeddedAssetsRead): void {
        setState((current) => withSyncedSelections({
            ...current,
            targetInfo: embedded.target,
            embeddedProfiles: embedded.profiles,
            embeddedSkills: embedded.skills,
        }));
    }

    // User-triggered failures are reported (inline + toastr) and rethrown so
    // the dev-log capture still observes them, matching prior semantics.
    async function runAssetAction(action: () => Promise<void>): Promise<void> {
        setState((current) => ({ ...current, saving: true, error: '' }));
        try {
            await action();
            applyEmbedded(actions.readEmbedded());
        } catch (error) {
            const message = actions.reportError(error);
            setState((current) => ({ ...current, error: message }));
            throw error;
        } finally {
            setState((current) => ({ ...current, saving: false }));
        }
    }

    async function embedSelectedProfile(): Promise<void> {
        if (!state.selectedProfileId) {
            throw new Error(tr('noEmbeddableProfiles'));
        }
        const profileId = state.selectedProfileId;
        await runAssetAction(async () => {
            const embeddedId = await actions.embedProfile(profileId);
            actions.toastSuccess(tr('embeddedProfile', { id: embeddedId }));
        });
    }

    async function embedSelectedSkill(): Promise<void> {
        const skill = state.skills.find((item) => item.key === state.selectedSkillKey) ?? null;
        if (!skill) {
            throw new Error(tr('selectSkillFirst'));
        }
        await runAssetAction(async () => {
            await actions.embedSkill(skill);
            actions.toastSuccess(tr('embeddedSkill', { name: skillOptionLabel(skill) }));
        });
    }

    async function removeProfileItem(item: EmbeddedProfileItem): Promise<void> {
        const profileId = item.profile.id;
        await runAssetAction(async () => {
            await actions.removeProfile(profileId);
            actions.toastSuccess(tr('removedEmbeddedProfile', { id: profileId }));
        });
    }

    async function removeSkillItem(item: EmbeddedSkillItem): Promise<void> {
        const skillName = item.skillName;
        await runAssetAction(async () => {
            await actions.removeSkill(skillName);
            actions.toastSuccess(tr('removedEmbeddedSkill', { name: skillName }));
        });
    }

    const embeddableProfiles = embeddableProfilesOf(state);
    const selectedSkill = state.skills.find((skill) => skill.key === state.selectedSkillKey) ?? null;
    const selectedProfileEmbedded = state.embeddedProfiles.some((item) => item.profile.id === state.selectedProfileId);
    const selectedSkillEmbedded = selectedSkill !== null
        && state.embeddedSkills.some((item) => item.skillName === selectedSkill.name);
    const targetInfo = state.targetInfo;
    const targetTypeLabel = !targetInfo
        ? ''
        : targetInfo.kind === 'preset' ? tr('targetPreset') : tr('targetCharacter');

    return (
        <div className="ttas-root ttas-embed-panel">
            <header className="ttas-embed-titlebar">
                <div className="ttas-embed-title-icon" dangerouslySetInnerHTML={{ __html: AGENT_TOGGLE_ICON }} />
                <div className="ttas-embed-title-copy">
                    <span>{targetTypeLabel || tr('agentAssets')}</span>
                    <h3>{tr('agentAssets')}</h3>
                    {targetInfo && <p>{targetInfo.name}</p>}
                </div>
                <button type="button" className="menu_button menu_button_icon ttas-embed-close" aria-label={tr('close')} onClick={onRequestClose}>
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </header>

            <main className="ttas-embed-body">
                {state.loading && !state.initialized ? (
                    <div className="ttas-embed-loading" role="status" aria-live="polite">
                        <i className="fa-solid fa-spinner fa-spin"></i>
                        <span>{tr('embedAssetPanelLoading')}</span>
                    </div>
                ) : (
                    <>
                        {targetInfo && (
                            <div className="ttas-embed-target">
                                <i className={`fa-solid ${targetInfo.kind === 'preset' ? 'fa-sliders' : 'fa-id-card'}`}></i>
                                <div>
                                    <span>{targetTypeLabel}</span>
                                    <strong>{targetInfo.name}</strong>
                                    {targetInfo.subtitle && <small>{targetInfo.subtitle}</small>}
                                </div>
                            </div>
                        )}

                        {state.error && (
                            <div className="ttas-embed-error" role="alert">
                                <i className="fa-solid fa-triangle-exclamation"></i>
                                <span>{state.error}</span>
                            </div>
                        )}

                        <section className="ttas-embed-card">
                            <div className="ttas-embed-section-title">
                                <i className="fa-solid fa-id-card-clip"></i>
                                <h4>{tr('profiles')}</h4>
                            </div>
                            <div className="ttas-embed-action-row">
                                <label className="ttas-field">
                                    <span>{tr('selectProfile')}</span>
                                    <select
                                        value={state.selectedProfileId}
                                        disabled={state.saving || embeddableProfiles.length === 0}
                                        onChange={(event) => {
                                            setState((current) => ({ ...current, selectedProfileId: event.target.value }));
                                        }}
                                    >
                                        {embeddableProfiles.map((profile) => (
                                            <option key={profile.id} value={profile.id}>{profile.displayName || profile.id}</option>
                                        ))}
                                    </select>
                                </label>
                                <button
                                    type="button"
                                    className="menu_button menu_button_icon ttas-primary-button"
                                    disabled={state.saving || !state.selectedProfileId}
                                    onClick={() => void embedSelectedProfile()}
                                >
                                    <i className={`fa-solid ${state.saving ? 'fa-spinner fa-spin' : 'fa-file-arrow-down'}`}></i>
                                    <span>{selectedProfileEmbedded ? tr('updateEmbeddedAsset') : tr('embedProfile')}</span>
                                </button>
                            </div>
                            {embeddableProfiles.length === 0 && <p className="ttas-embed-empty">{tr('noEmbeddableProfiles')}</p>}
                        </section>

                        <section className="ttas-embed-card">
                            <div className="ttas-embed-section-title">
                                <i className="fa-solid fa-book-bookmark"></i>
                                <h4>{tr('skills')}</h4>
                            </div>
                            <div className="ttas-embed-action-row">
                                <label className="ttas-field">
                                    <span>{tr('selectSkill')}</span>
                                    <select
                                        value={state.selectedSkillKey}
                                        disabled={state.saving || state.skills.length === 0}
                                        onChange={(event) => {
                                            setState((current) => ({ ...current, selectedSkillKey: event.target.value }));
                                        }}
                                    >
                                        {state.skills.map((skill) => (
                                            <option key={skill.key} value={skill.key}>{skillOptionLabel(skill)}</option>
                                        ))}
                                    </select>
                                </label>
                                <button
                                    type="button"
                                    className="menu_button menu_button_icon ttas-primary-button"
                                    disabled={state.saving || !selectedSkill}
                                    onClick={() => void embedSelectedSkill()}
                                >
                                    <i className={`fa-solid ${state.saving ? 'fa-spinner fa-spin' : 'fa-file-zipper'}`}></i>
                                    <span>{selectedSkillEmbedded ? tr('updateEmbeddedAsset') : tr('embedSkill')}</span>
                                </button>
                            </div>
                            {state.skills.length === 0 && <p className="ttas-embed-empty">{tr('noSkillsInstalled')}</p>}
                        </section>

                        <section className="ttas-embed-card ttas-embed-current">
                            <div className="ttas-embed-section-title">
                                <i className="fa-solid fa-layer-group"></i>
                                <h4>{tr('embeddedAssets')}</h4>
                            </div>

                            <div className="ttas-embedded-group">
                                <h5>{tr('embeddedProfiles')}</h5>
                                {state.embeddedProfiles.length > 0 ? (
                                    <div className="ttas-embedded-list">
                                        {state.embeddedProfiles.map((item) => (
                                            <div key={item.profile.id} className="ttas-embedded-item">
                                                <i className="fa-solid fa-id-card-clip"></i>
                                                <div>
                                                    <strong>{profileDisplayName(item)}</strong>
                                                    <span>{item.profile.id}</span>
                                                </div>
                                                <button
                                                    type="button"
                                                    className="menu_button menu_button_icon ttas-danger-button"
                                                    title={tr('removeEmbeddedAsset')}
                                                    aria-label={tr('removeEmbeddedAsset')}
                                                    disabled={state.saving}
                                                    onClick={() => void removeProfileItem(item)}
                                                >
                                                    <i className="fa-solid fa-xmark"></i>
                                                </button>
                                            </div>
                                        ))}
                                    </div>
                                ) : (
                                    <p className="ttas-embed-empty">{tr('noEmbeddedProfiles')}</p>
                                )}
                            </div>

                            <div className="ttas-embedded-group">
                                <h5>{tr('embeddedSkills')}</h5>
                                {state.embeddedSkills.length > 0 ? (
                                    <div className="ttas-embedded-list">
                                        {state.embeddedSkills.map((item) => (
                                            <div key={item.skillName} className="ttas-embedded-item">
                                                <i className="fa-solid fa-book-bookmark"></i>
                                                <div>
                                                    <strong>{item.skillName}</strong>
                                                    <span>{embeddedSkillSubtitle(item)}</span>
                                                </div>
                                                <button
                                                    type="button"
                                                    className="menu_button menu_button_icon ttas-danger-button"
                                                    title={tr('removeEmbeddedAsset')}
                                                    aria-label={tr('removeEmbeddedAsset')}
                                                    disabled={state.saving}
                                                    onClick={() => void removeSkillItem(item)}
                                                >
                                                    <i className="fa-solid fa-xmark"></i>
                                                </button>
                                            </div>
                                        ))}
                                    </div>
                                ) : (
                                    <p className="ttas-embed-empty">{tr('noEmbeddedSkills')}</p>
                                )}
                            </div>
                        </section>
                    </>
                )}
            </main>
        </div>
    );
}
