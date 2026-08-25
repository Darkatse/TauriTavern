import { DEFAULT_PROFILE_ID } from './constants.js';
import { prettyJson } from './host-api.js';
import { findModelTargetForBinding, modelTargetIdFromConnectionRef } from './model-target-connection.js';
import { sanitizePortableAgentProfile } from '../../../tauritavern/agent/agent-profile-portable.js';
import { normalizeProfileForSave } from './profile-model';
import {
    PROFILE_EXPORT_CONTENT_TYPE,
    activeProfileIdOf,
    isBuiltinProfile,
    type AgentSystemPanelControllerDeps,
    type AgentSystemPanelSnapshot,
} from './AgentSystemPanelContract';

export type AgentSystemPanelPersistenceContext = {
    deps: AgentSystemPanelControllerDeps;
    getSnapshot: () => AgentSystemPanelSnapshot;
    commit: (patch: Partial<AgentSystemPanelSnapshot>) => void;
    isDisposed: () => boolean;
    saveSettingsPatch: (patch: Partial<AgentSystemPanelSnapshot['settings']>) => Promise<void>;
    selectProfile: (profileId: string, options?: { persistEditing?: boolean }) => Promise<void>;
    reportError: (error: unknown) => void;
};

export type AgentSystemPanelPersistence = {
    refreshProfiles: (options?: { repairListIssues?: boolean }) => Promise<void>;
    saveProfile: () => Promise<void>;
    deleteProfile: () => Promise<void>;
    exportSelectedProfile: () => Promise<void>;
};

/**
 * Profile persistence operations: list refresh with file-issue repair, save
 * (binding -> profile -> settings -> reload), delete, and portable export.
 */
export function createAgentSystemPanelPersistence(
    context: AgentSystemPanelPersistenceContext,
): AgentSystemPanelPersistence {
    const { deps } = context;

    async function refreshProfiles(options: { repairListIssues?: boolean } = {}): Promise<void> {
        const profilesApi = deps.getProfilesApi();
        const result = await profilesApi.list();
        if (context.isDisposed()) {
            return;
        }
        context.commit({ profiles: Array.isArray(result?.profiles) ? result.profiles : [] });
        const issues = Array.isArray(result?.issues) ? result.issues : [];
        if (options.repairListIssues === false || issues.length === 0) {
            return;
        }
        const repaired = await repairProfileListIssues(issues);
        if (context.isDisposed()) {
            return;
        }
        if (repaired) {
            const refreshed = await profilesApi.list();
            if (context.isDisposed()) {
                return;
            }
            context.commit({ profiles: Array.isArray(refreshed?.profiles) ? refreshed.profiles : [] });
        }
    }

    async function repairProfileListIssues(issues: TauriTavernAgentProfileStorageIssue[]): Promise<boolean> {
        const profilesApi = deps.getProfilesApi();
        if (typeof profilesApi.repairFile !== 'function') {
            throw new Error(deps.tr('hostAgentProfileApiUnavailable'));
        }

        let repaired = false;
        for (const issue of issues) {
            const profileId = String(issue?.profileId || '').trim();
            const action = String(issue?.recommendedAction || '').trim();
            if (!profileId) {
                throw new Error('Agent profile repair issue is missing profileId');
            }
            if (!action) {
                deps.notifyWarning(deps.tr('agentProfileManualRepairRequired', {
                    id: profileId,
                    error: String(issue?.message || ''),
                }));
                continue;
            }
            if (action === 'delete') {
                const message = deps.tr('deleteCorruptAgentProfileConfirm', {
                    id: profileId,
                    error: String(issue?.message || ''),
                });
                if (!await deps.confirmAction(message)) {
                    continue;
                }
                if (context.isDisposed()) {
                    return repaired;
                }
                try {
                    await profilesApi.repairFile({ profileId, action });
                } catch (error) {
                    context.reportError(error);
                    continue;
                }
                deps.notifyWarning(deps.tr('deletedCorruptAgentProfile', { id: profileId }));
                repaired = true;
                continue;
            }
            if (action === 'normalizeIdentity') {
                try {
                    await profilesApi.repairFile({ profileId, action });
                } catch (error) {
                    context.reportError(error);
                    continue;
                }
                deps.notifyWarning(deps.tr('normalizedAgentProfileIdentity', { id: profileId }));
                repaired = true;
                continue;
            }
            throw new Error(`Unsupported Agent profile repair action: ${action}`);
        }
        return repaired;
    }

    // Materialize the Model Target binding as an LLM connection before the
    // profile references it; save order stays binding -> profile -> settings.
    async function persistProfileModelBinding(profile: TauriTavernAgentProfileDefinition): Promise<void> {
        if (profile.model.mode !== 'connectionRef' || !modelTargetIdFromConnectionRef(profile.model.connectionRef ?? '')) {
            return;
        }
        const modelTargets = deps.listModelTargets();
        context.commit({ modelTargets });
        const target = findModelTargetForBinding(modelTargets, profile.model);
        if (!target) {
            return;
        }
        await deps.saveModelTargetConnection(target);
    }

    function profileDraftHasUnsavedChanges(savedProfile: TauriTavernAgentProfileDefinition): boolean {
        return prettyJson(normalizeProfileForSave(context.getSnapshot().draft)) !== prettyJson(savedProfile);
    }

    async function saveProfile(): Promise<void> {
        const snapshot = context.getSnapshot();
        if (isBuiltinProfile(snapshot.draft)) {
            throw new Error(deps.tr('agentProfileBuiltInEdit'));
        }
        if (snapshot.externalProfileChangePending) {
            throw new Error(deps.tr('agentProfileExternalChangeSaveBlocked'));
        }
        context.commit({ saving: true });
        try {
            const profile = normalizeProfileForSave(snapshot.draft);
            const wasActiveProfile = activeProfileIdOf(snapshot.settings) === profile.id;
            await persistProfileModelBinding(profile);
            await deps.getProfilesApi().save({ profile });
            deps.notifySuccess(deps.tr('agentProfileSaved'));
            try {
                await refreshProfiles();
            } catch (error) {
                context.reportError(error);
            }
            if (context.isDisposed()) {
                return;
            }
            const settingsPatch: Partial<AgentSystemPanelSnapshot['settings']> = { editingProfileId: profile.id };
            if (profile.run.directRunnable === false && wasActiveProfile) {
                settingsPatch.activeProfileId = DEFAULT_PROFILE_ID;
            }
            await context.saveSettingsPatch(settingsPatch);
            if (context.isDisposed()) {
                return;
            }
            await context.selectProfile(profile.id, { persistEditing: false });
            if (context.isDisposed()) {
                return;
            }
            if (settingsPatch.activeProfileId) {
                deps.notifyWarning(deps.tr('activeProfileResetToDefault'));
            }
        } catch (error) {
            context.reportError(error);
            throw error;
        } finally {
            context.commit({ saving: false });
        }
    }

    async function deleteProfile(): Promise<void> {
        const snapshot = context.getSnapshot();
        if (isBuiltinProfile(snapshot.draft)) {
            throw new Error(deps.tr('agentProfileBuiltInDelete'));
        }
        const id = snapshot.draft.id;
        if (!await deps.confirmAction(deps.tr('deleteAgentProfileConfirm', { id }))) {
            return;
        }
        if (context.isDisposed()) {
            return;
        }
        await deps.getProfilesApi().delete({ profileId: id });
        deps.notifySuccess(deps.tr('deletedProfile', { id }));
        if (context.isDisposed()) {
            return;
        }
        const settingsPatch: Partial<AgentSystemPanelSnapshot['settings']> = {
            editingProfileId: DEFAULT_PROFILE_ID,
            ...(activeProfileIdOf(context.getSnapshot().settings) === id ? { activeProfileId: DEFAULT_PROFILE_ID } : {}),
        };
        const refresh = refreshProfiles().catch(context.reportError);
        const reconciled = await Promise.allSettled([
            context.saveSettingsPatch(settingsPatch),
            context.selectProfile(DEFAULT_PROFILE_ID, { persistEditing: false }),
        ]);
        await refresh;
        const failure = reconciled.find((result): result is PromiseRejectedResult => result.status === 'rejected');
        if (failure) {
            throw failure.reason;
        }
    }

    async function exportSelectedProfile(): Promise<void> {
        const profileId = context.getSnapshot().editingProfileId || DEFAULT_PROFILE_ID;
        const result = await deps.getProfilesApi().load({ profileId });
        const profile = result?.profile;
        if (!profile) {
            throw new Error(deps.tr('agentProfileNotFound', { id: profileId }));
        }
        if (profileId !== DEFAULT_PROFILE_ID && profileDraftHasUnsavedChanges(profile)) {
            throw new Error(deps.tr('agentProfileExportSaveFirst'));
        }

        const portableProfile = sanitizePortableAgentProfile(profile);
        const blob = new Blob([`${prettyJson(portableProfile)}\n`], { type: PROFILE_EXPORT_CONTENT_TYPE });
        const downloadResult = await deps.downloadBlob(blob, `${profile.id}.agent-profile.json`);
        if (downloadResult?.mode !== 'ios-native-share' || downloadResult.completed === true) {
            deps.notifySuccess(deps.tr('exportedProfile', { id: profile.id }));
        }
    }

    return {
        refreshProfiles,
        saveProfile,
        deleteProfile,
        exportSelectedProfile,
    };
}
