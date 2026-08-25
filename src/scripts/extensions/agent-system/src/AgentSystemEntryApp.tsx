import { useSyncExternalStore } from 'react';

import { DEFAULT_AGENT_PROFILE_ID } from '../../../tauritavern/agent/agent-system-settings.js';
import type { AgentSystemEntryController } from './AgentSystemEntryController';
import type { AgentSystemTr } from './i18n';

export type AgentSystemEntryAppProps = {
    controller: AgentSystemEntryController;
    tr: AgentSystemTr;
    onOpenPanel: () => void;
};

export function AgentSystemEntryApp({ controller, tr, onOpenPanel }: AgentSystemEntryAppProps) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const { settings, loading } = snapshot;
    const activeProfileId = settings.activeProfileId || DEFAULT_AGENT_PROFILE_ID;
    const activeProfileOptions = snapshot.profiles.filter((profile) => profile.directRunnable !== false);

    // The controller reports action failures; UI handlers do not await them.
    return (
        <div id="agent_system_settings" className="ttas-root">
            <div className="inline-drawer">
                <div className="inline-drawer-toggle inline-drawer-header">
                    <b>{tr('agentSystem')}</b>
                    <div className="inline-drawer-icon fa-solid fa-circle-chevron-down down"></div>
                </div>
                <div className="inline-drawer-content">
                    <div className="ttas-entry">
                        <button
                            type="button"
                            className={`menu_button menu_button_icon${settings.agentModeEnabled ? ' active' : ''}`}
                            disabled={loading}
                            onClick={() => void controller.toggleAgentMode()}
                        >
                            <i className={`fa-solid ${settings.agentModeEnabled ? 'fa-toggle-on' : 'fa-toggle-off'}`}></i>
                            <span>{settings.agentModeEnabled ? tr('agentModeOn') : tr('agentModeOff')}</span>
                        </button>
                        <button
                            type="button"
                            className={`menu_button menu_button_icon${settings.chatInputToggleHidden ? ' active' : ''}`}
                            aria-pressed={settings.chatInputToggleHidden}
                            disabled={loading}
                            onClick={() => void controller.toggleChatInputToggleVisibility()}
                        >
                            <i className={`fa-solid ${settings.chatInputToggleHidden ? 'fa-eye' : 'fa-eye-slash'}`}></i>
                            <span>{settings.chatInputToggleHidden ? tr('showChatInputToggle') : tr('hideChatInputToggle')}</span>
                        </button>
                        <label className="ttas-field ttas-entry-active-profile">
                            <span>{tr('activeProfile')}</span>
                            <select
                                value={activeProfileId}
                                disabled={loading || activeProfileOptions.length === 0}
                                onChange={(event) => void controller.setActiveProfile(event.target.value)}
                            >
                                {activeProfileOptions.map((profile) => (
                                    <option key={profile.id} value={profile.id}>{profile.displayName || profile.id}</option>
                                ))}
                            </select>
                        </label>
                        <button type="button" className="menu_button menu_button_icon" onClick={onOpenPanel}>
                            <i className="fa-solid fa-up-right-from-square"></i>
                            <span>{tr('openAgentSystem')}</span>
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}
