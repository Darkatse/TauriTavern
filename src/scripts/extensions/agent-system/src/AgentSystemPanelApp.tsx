import { useLayoutEffect, useRef, useSyncExternalStore } from 'react';

import {
    PANEL_TABS,
    PROFILE_EDIT_MODES,
    activeProfileIdOf,
    activeProfileOptions,
    isBuiltinProfile,
    profileSectionsForMode,
    type Tr,
} from './AgentSystemPanelContract';
import { profileStatsView } from './AgentSystemPanelView';
import type { AgentSystemPanelController } from './AgentSystemPanelController';
import type { RunHistoryController } from './RunHistoryController';
import type { RunRetentionController } from './RunRetentionController';
import { ProfileIdentitySection, ProfileBindingSection } from './ProfileIdentityBindingSections';
import {
    ProfileContextSection,
    ProfileMainDelegationSection,
    ProfilePromptSection,
    ProfileRunSection,
    ProfileSubAgentAccessSection,
} from './ProfilePolicyToolsSections';
import { ProfileToolsSection } from './ProfileToolsSection';
import {
    ProfileJsonSection,
    ProfileOutputSection,
    ProfileSkillsSection,
    ProfileWorkspaceSection,
} from './ProfileResourcesOutputSections';
import { RunHistoryPanel } from './RunHistoryPanel';

export type AgentSystemPanelAppProps = {
    controller: AgentSystemPanelController;
    runHistory: RunHistoryController;
    runRetention: RunRetentionController;
    tr: Tr;
    onRequestClose: () => void;
};

export function AgentSystemPanelApp({ controller, runHistory, runRetention, tr, onRequestClose }: AgentSystemPanelAppProps) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const { draft, settings, profiles } = snapshot;
    const activeProfileId = activeProfileIdOf(settings);
    const runnableProfiles = activeProfileOptions(profiles);
    const builtin = isBuiltinProfile(draft);
    const visibleSections = profileSectionsForMode(snapshot.profileEditMode);
    const stats = profileStatsView({
        draft,
        toolIds: snapshot.toolIds,
        profileEditMode: snapshot.profileEditMode,
        modelTargets: snapshot.modelTargets,
    }, tr);

    const rootRef = useRef<HTMLDivElement>(null);
    const { activeProfileSectionId, profileSectionScrollRequest } = snapshot;
    // Scroll only after React commits the selected section.
    useLayoutEffect(() => {
        if (profileSectionScrollRequest === 0) {
            return;
        }
        const section = rootRef.current?.querySelector(`[data-ttas-profile-section="${activeProfileSectionId}"]`);
        section?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
    }, [profileSectionScrollRequest, activeProfileSectionId]);

    return (
        <div ref={rootRef} className="ttas-root ttas-panel-root">
            <header className="ttas-titlebar">
                <div className="ttas-titlebar-main">
                    <div className="ttas-title-icon" aria-hidden="true">
                        <i className="fa-solid fa-atom"></i>
                    </div>
                    <div className="ttas-title-copy">
                        <div className="ttas-eyebrow">{tr('tauriTavernAgent')}</div>
                        <h3>{tr('agentSystem')}</h3>
                    </div>
                </div>
                <button type="button" className="menu_button menu_button_icon ttas-close-button" title={tr('close')} onClick={onRequestClose}>
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </header>

            {snapshot.loading && !snapshot.initialized ? (
                <div className="ttas-loading">{tr('loadingAgentSystem')}</div>
            ) : (
                <div className="ttas-panel-body">
                    {snapshot.error && (
                        <div className="ttas-error">
                            <i className="fa-solid fa-triangle-exclamation"></i>
                            <pre>{snapshot.error}</pre>
                        </div>
                    )}

                    <nav className="ttas-tabs">
                        {PANEL_TABS.map((tab) => (
                            <button
                                key={tab.id}
                                type="button"
                                className={`menu_button${settings.activeTab === tab.id ? ' active' : ''}`}
                                onClick={() => void controller.setTab(tab.id)}
                            >
                                <i className={`fa-solid ${tab.icon}`}></i>
                                <span>{tr(tab.labelKey)}</span>
                            </button>
                        ))}
                    </nav>

                    {settings.activeTab === 'profiles' ? (
                        <section key="profiles" className="ttas-panel">
                            <div className="ttas-profile-layout">
                                <aside className="ttas-list ttas-side-list">
                                    <div className="ttas-list-header">
                                        <h4>{tr('profiles')}</h4>
                                        <span>{tr('profileCount', { count: profiles.length })}</span>
                                    </div>
                                    <label className="ttas-field ttas-side-active-profile">
                                        <span>{tr('activeProfile')}</span>
                                        <select
                                            value={activeProfileId}
                                            onChange={(event) => void controller.setActiveProfile(event.target.value)}
                                        >
                                            {runnableProfiles.map((profile) => (
                                                <option key={profile.id} value={profile.id}>{profile.displayName || profile.id}</option>
                                            ))}
                                        </select>
                                    </label>
                                    {profiles.map((profile) => (
                                        <button
                                            key={profile.id}
                                            type="button"
                                            className={[
                                                snapshot.editingProfileId === profile.id ? 'active' : '',
                                                activeProfileId === profile.id ? 'is-run-profile' : '',
                                            ].filter(Boolean).join(' ')}
                                            onClick={() => void controller.selectProfile(profile.id)}
                                        >
                                            <strong>{profile.displayName}</strong>
                                            <span>
                                                {profile.id}
                                                {activeProfileId === profile.id && (
                                                    <em className="ttas-active-profile-badge">{tr('activeProfileShort')}</em>
                                                )}
                                            </span>
                                            {profile.description && <small>{profile.description}</small>}
                                        </button>
                                    ))}
                                </aside>
                                <nav className="ttas-section-rail" aria-label={tr('profileSections')}>
                                    {visibleSections.map((section) => (
                                        <button
                                            key={section.id}
                                            type="button"
                                            className={`ttas-section-jump${activeProfileSectionId === section.id ? ' active' : ''}`}
                                            title={tr(section.labelKey)}
                                            onClick={() => controller.scrollToProfileSection(section.id)}
                                        >
                                            <i className={`fa-solid ${section.icon}`}></i>
                                            <span>{tr(section.labelKey)}</span>
                                        </button>
                                    ))}
                                </nav>
                                <div className="ttas-editor">
                                    <div className="ttas-mobile-profile-controls">
                                        <label className="ttas-field">
                                            <span>{tr('editingProfile')}</span>
                                            <select
                                                value={snapshot.editingProfileId}
                                                onChange={(event) => void controller.selectProfile(event.target.value)}
                                            >
                                                {profiles.map((profile) => (
                                                    <option key={profile.id} value={profile.id}>{profile.displayName}</option>
                                                ))}
                                            </select>
                                        </label>
                                        <label className="ttas-field">
                                            <span>{tr('activeProfile')}</span>
                                            <select
                                                value={activeProfileId}
                                                onChange={(event) => void controller.setActiveProfile(event.target.value)}
                                            >
                                                {runnableProfiles.map((profile) => (
                                                    <option key={profile.id} value={profile.id}>{profile.displayName || profile.id}</option>
                                                ))}
                                            </select>
                                        </label>
                                    </div>

                                    <div className="ttas-editor-hero">
                                        <div className="ttas-hero-copy">
                                            <div className="ttas-eyebrow">{tr('profileSummary')}</div>
                                            <h4>{draft.displayName || draft.id}</h4>
                                            <p>{draft.id}</p>
                                        </div>
                                        <div className="ttas-editor-actions">
                                            <button type="button" className="menu_button menu_button_icon" onClick={() => controller.newProfile()}>
                                                <i className="fa-solid fa-plus"></i>
                                                <span>{tr('new')}</span>
                                            </button>
                                            <button type="button" className="menu_button menu_button_icon" onClick={() => controller.copyProfile()}>
                                                <i className="fa-solid fa-copy"></i>
                                                <span>{tr('copy')}</span>
                                            </button>
                                            <button
                                                type="button"
                                                className="menu_button menu_button_icon"
                                                disabled={snapshot.saving}
                                                onClick={() => void controller.exportSelectedProfile()}
                                            >
                                                <i className="fa-solid fa-file-export"></i>
                                                <span>{tr('export')}</span>
                                            </button>
                                            <button
                                                type="button"
                                                className="menu_button menu_button_icon ttas-primary-button"
                                                disabled={snapshot.saving || builtin}
                                                onClick={() => void controller.saveProfile()}
                                            >
                                                <i className={`fa-solid ${snapshot.saving ? 'fa-spinner fa-spin' : 'fa-floppy-disk'}`}></i>
                                                <span>{tr('save')}</span>
                                            </button>
                                            <button
                                                type="button"
                                                className="menu_button menu_button_icon ttas-danger-button"
                                                disabled={builtin}
                                                onClick={() => void controller.deleteProfile()}
                                            >
                                                <i className="fa-solid fa-trash-can"></i>
                                                <span>{tr('delete')}</span>
                                            </button>
                                        </div>
                                        <div className="ttas-profile-mode-switch" aria-label={tr('profileView')}>
                                            {PROFILE_EDIT_MODES.map((mode) => (
                                                <button
                                                    key={mode.id}
                                                    type="button"
                                                    className={`menu_button menu_button_icon${snapshot.profileEditMode === mode.id ? ' active' : ''}`}
                                                    onClick={() => controller.setProfileEditMode(mode.id)}
                                                >
                                                    <i className={`fa-solid ${mode.icon}`}></i>
                                                    <span>{tr(mode.labelKey)}</span>
                                                </button>
                                            ))}
                                        </div>
                                        <div className="ttas-stat-grid">
                                            {stats.map((stat) => (
                                                <div key={stat.label} className="ttas-stat">
                                                    <i className={`fa-solid ${stat.icon}`}></i>
                                                    <span>{stat.label}</span>
                                                    <strong>{stat.value}</strong>
                                                </div>
                                            ))}
                                        </div>
                                    </div>

                                    <ProfileIdentitySection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfileBindingSection snapshot={snapshot} controller={controller} tr={tr} />
                                    {snapshot.profileEditMode === 'main' ? (
                                        <ProfileMainDelegationSection snapshot={snapshot} controller={controller} tr={tr} />
                                    ) : (
                                        <ProfileSubAgentAccessSection snapshot={snapshot} controller={controller} tr={tr} />
                                    )}
                                    <ProfileRunSection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfileContextSection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfilePromptSection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfileToolsSection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfileSkillsSection snapshot={snapshot} controller={controller} tr={tr} />
                                    <ProfileWorkspaceSection snapshot={snapshot} controller={controller} tr={tr} />
                                    {snapshot.profileEditMode === 'main' && (
                                        <ProfileOutputSection snapshot={snapshot} controller={controller} tr={tr} />
                                    )}
                                    <ProfileJsonSection snapshot={snapshot} controller={controller} tr={tr} />
                                </div>
                            </div>
                        </section>
                    ) : settings.activeTab === 'runs' ? (
                        <section key="runs" className="ttas-panel">
                            <RunHistoryPanel controller={runHistory} retention={runRetention} tr={tr} />
                        </section>
                    ) : null}
                </div>
            )}
        </div>
    );
}
