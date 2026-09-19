import type { AgentSystemPanelController } from './AgentSystemPanelController';
import { isBuiltinProfile, type AgentSystemPanelSnapshot, type Tr } from './AgentSystemPanelContract';
import {
    availablePresetOptions,
    hasExternalModelBinding,
    isProfileRuntimeStateCurrent,
    modelSummaryLabel,
    modelTargetBadges,
    presetSummaryLabel,
    profileConfigurationWarnings,
    selectedModelTarget,
} from './AgentSystemPanelView';

export type ProfileSectionProps = {
    snapshot: AgentSystemPanelSnapshot;
    controller: AgentSystemPanelController;
    tr: Tr;
};

export function ProfileIdentitySection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="identity">
            <div className="ttas-section-title">
                <i className="fa-solid fa-fingerprint"></i>
                <h4>{tr('identity')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-field">
                    <span>{tr('profileId')}</span>
                    <input
                        className="text_pole"
                        value={draft.id}
                        disabled={builtin}
                        onChange={(event) => controller.setIdentityField('id', event.target.value)}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('displayName')}</span>
                    <input
                        className="text_pole"
                        value={draft.displayName}
                        disabled={builtin}
                        onChange={(event) => controller.setIdentityField('displayName', event.target.value)}
                    />
                </label>
                <label className="ttas-field ttas-span-2">
                    <span>{tr('description')}</span>
                    <input
                        className="text_pole"
                        value={draft.description ?? ''}
                        disabled={builtin}
                        onChange={(event) => controller.setIdentityField('description', event.target.value)}
                    />
                </label>
            </div>
        </div>
    );
}

export function ProfileBindingSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft, presetOptions, modelTargets } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const presetOptionsWithSelected = availablePresetOptions(presetOptions, draft);
    const selectedTarget = selectedModelTarget(modelTargets, draft);
    const selectedTargetId = selectedTarget?.id || '';
    const externalModelBinding = hasExternalModelBinding(draft, modelTargets);
    const warnings = profileConfigurationWarnings({
        draft,
        toolCatalogDiagnostics: snapshot.toolCatalogDiagnostics,
        profileHealth: snapshot.profileHealth,
        isRuntimeStateCurrent: isProfileRuntimeStateCurrent(draft, snapshot.profileRuntimeStateJson),
        profileDiagnosticError: snapshot.profileDiagnosticError,
        profilePreviewError: snapshot.profilePreviewError,
        presetOptions,
        modelTargets,
    }, tr);

    return (
        <div className="ttas-section" data-ttas-profile-section="binding">
            <div className="ttas-section-title">
                <i className="fa-solid fa-sliders"></i>
                <h4>{tr('presetAndModel')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-field">
                    <span>{tr('presetSource')}</span>
                    <select
                        value={draft.preset.mode}
                        disabled={builtin}
                        onChange={(event) => controller.setPresetMode(event.target.value)}
                    >
                        <option value="currentPromptSnapshot">{tr('currentPromptPreset')}</option>
                        <option value="ref">{tr('savedChatCompletionPreset')}</option>
                        <option value="none">{tr('noPromptPreset')}</option>
                    </select>
                </label>
                {draft.preset.mode === 'ref' ? (
                    <label className="ttas-field">
                        <span>{tr('savedPreset')}</span>
                        <select
                            value={draft.preset.ref?.name || ''}
                            disabled={builtin}
                            onChange={(event) => controller.setPresetName(event.target.value)}
                        >
                            {presetOptionsWithSelected.length === 0 && <option value="">{tr('none')}</option>}
                            {presetOptionsWithSelected.map((name) => (
                                <option key={name} value={name}>{name}</option>
                            ))}
                        </select>
                    </label>
                ) : (
                    <div className="ttas-binding-status">
                        <i className="fa-solid fa-scroll"></i>
                        <strong>{presetSummaryLabel(draft, tr)}</strong>
                        <span>{tr('preset')}</span>
                    </div>
                )}

                <label className="ttas-field">
                    <span>{tr('modelSource')}</span>
                    <select
                        value={draft.model.mode}
                        disabled={builtin}
                        onChange={(event) => controller.setModelMode(event.target.value)}
                    >
                        <option value="currentPromptSnapshot">{tr('currentChatModel')}</option>
                        {draft.model.mode === 'requiresConfiguration' && (
                            <option value="requiresConfiguration">{tr('modelRequiresConfiguration')}</option>
                        )}
                        <option value="connectionRef" disabled={modelTargets.length === 0 && draft.model.mode !== 'connectionRef'}>
                            {tr('savedModelTarget')}
                        </option>
                    </select>
                </label>
                {draft.model.mode === 'connectionRef' && modelTargets.length > 0 ? (
                    <label className="ttas-field">
                        <span>{tr('savedModel')}</span>
                        <select
                            value={selectedTargetId}
                            disabled={builtin}
                            onChange={(event) => controller.setModelTarget(event.target.value)}
                        >
                            {externalModelBinding && <option value="">{modelSummaryLabel(draft, modelTargets, tr)}</option>}
                            {modelTargets.map((target) => (
                                <option key={target.id} value={target.id}>{target.name || target.model}</option>
                            ))}
                        </select>
                    </label>
                ) : (
                    <div className="ttas-binding-status">
                        <i className="fa-solid fa-microchip"></i>
                        <strong>{modelSummaryLabel(draft, modelTargets, tr)}</strong>
                        <span>{tr('model')}</span>
                    </div>
                )}

                {selectedTarget && (
                    <div className="ttas-binding-summary ttas-span-2">
                        <i className="fa-solid fa-plug-circle-check"></i>
                        <div>
                            <strong>{selectedTarget.name || selectedTarget.model}</strong>
                            <div className="ttas-tool-badge-row">
                                {modelTargetBadges(selectedTarget).map((badge) => (
                                    <span key={badge}>{badge}</span>
                                ))}
                            </div>
                        </div>
                    </div>
                )}
                {!selectedTarget && externalModelBinding && (
                    <div className="ttas-binding-summary ttas-binding-warning ttas-span-2">
                        <i className="fa-solid fa-link"></i>
                        <div>
                            <strong>{draft.model.connectionRef}</strong>
                            <span>{draft.model.modelId}</span>
                        </div>
                    </div>
                )}
                {warnings.length > 0 && (
                    <div className="ttas-binding-summary ttas-binding-warning ttas-span-2">
                        <i className="fa-solid fa-triangle-exclamation"></i>
                        <div>
                            <strong>{tr('agentProfileConfigurationWarnings')}</strong>
                            {warnings.map((warning) => (
                                <span key={warning}>{warning}</span>
                            ))}
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
