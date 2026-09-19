import type { AgentSystemPanelController } from './AgentSystemPanelController';
import {
    isBuiltinProfile,
    isCallableAsHandoffTarget,
    isCallableAsSubAgent,
    isSubAgentOnly,
    parseNumberInput,
    type AgentSystemPanelSnapshot,
    type Tr,
} from './AgentSystemPanelContract';
import {
    agentSystemPromptEditorValue,
    agentSystemPromptPlaceholder,
    isProfileRuntimeStateCurrent,
} from './AgentSystemPanelView';

export type ProfileSectionProps = {
    snapshot: AgentSystemPanelSnapshot;
    controller: AgentSystemPanelController;
    tr: Tr;
};

export function ProfileMainDelegationSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const callableHandoffTarget = isCallableAsHandoffTarget(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="main-delegation">
            <div className="ttas-section-title">
                <i className="fa-solid fa-diagram-project"></i>
                <h4>{tr('mainAgentControl')}</h4>
            </div>
            <div className="ttas-delegation-panel">
                <label className="ttas-switch-row">
                    <input
                        type="checkbox"
                        checked={draft.delegation.canDelegate}
                        disabled={builtin}
                        onChange={(event) => controller.setCanDelegate(event.target.checked)}
                    />
                    <span>
                        <strong>{tr('delegateToSubAgents')}</strong>
                        <small>{tr('delegateToSubAgentsHint')}</small>
                    </span>
                </label>
                {draft.delegation.canDelegate && (
                    <div className="ttas-form-grid ttas-delegation-controls">
                        <label className="ttas-field">
                            <span>{tr('maxConcurrentSubAgents')}</span>
                            <input
                                className="text_pole"
                                type="number"
                                min="1"
                                value={draft.delegation.maxConcurrentInvocations}
                                disabled={builtin}
                                onChange={(event) => controller.setDelegationLimit('maxConcurrentInvocations', parseNumberInput(event.target.value))}
                            />
                        </label>
                        <label className="ttas-field">
                            <span>{tr('maxSubAgentTasks')}</span>
                            <input
                                className="text_pole"
                                type="number"
                                min="1"
                                value={draft.delegation.maxInvocationsPerRun}
                                disabled={builtin}
                                onChange={(event) => controller.setDelegationLimit('maxInvocationsPerRun', parseNumberInput(event.target.value))}
                            />
                        </label>
                    </div>
                )}
                <label className="ttas-switch-row">
                    <input
                        type="checkbox"
                        checked={draft.delegation.canHandoff}
                        disabled={builtin}
                        onChange={(event) => controller.setCanHandoff(event.target.checked)}
                    />
                    <span>
                        <strong>{tr('allowAgentHandoff')}</strong>
                        <small>{tr('allowAgentHandoffHint')}</small>
                    </span>
                </label>
                {draft.delegation.canHandoff && (
                    <div className="ttas-form-grid ttas-delegation-controls">
                        <label className="ttas-field">
                            <span>{tr('maxHandoffDepth')}</span>
                            <input
                                className="text_pole"
                                type="number"
                                min="1"
                                value={draft.delegation.maxHandoffDepth}
                                disabled={builtin}
                                onChange={(event) => controller.setDelegationLimit('maxHandoffDepth', parseNumberInput(event.target.value))}
                            />
                        </label>
                    </div>
                )}
                <label className="ttas-switch-row">
                    <input
                        type="checkbox"
                        checked={callableHandoffTarget}
                        disabled={builtin}
                        onChange={(event) => controller.setCallableAsHandoffTarget(event.target.checked)}
                    />
                    <span>
                        <strong>{tr('callableHandoffTargetToggle')}</strong>
                        <small>{tr('callableHandoffTargetHint')}</small>
                    </span>
                </label>
                {callableHandoffTarget && (
                    <div className="ttas-form-grid ttas-delegation-controls">
                        <label className="ttas-field ttas-span-2">
                            <span>{tr('agentFacingDescription')}</span>
                            <textarea
                                className="text_pole textarea_compact"
                                rows={4}
                                value={draft.delegation.descriptionForAgents ?? ''}
                                disabled={builtin}
                                placeholder={draft.description ?? ''}
                                onChange={(event) => controller.setDelegationDescription(event.target.value)}
                            ></textarea>
                            <small className="ttas-field-hint">
                                <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                                <span>{tr('agentFacingDescriptionHint')}</span>
                            </small>
                        </label>
                        <label className="ttas-field ttas-span-2">
                            <span>{tr('allowedCallers')}</span>
                            <input
                                className="text_pole"
                                value={draft.delegation.allowedCallersCsv ?? ''}
                                disabled={builtin}
                                onChange={(event) => controller.setAllowedCallersCsv(event.target.value)}
                            />
                            <small className="ttas-field-hint">
                                <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                                <span>{tr('allowedCallersHint')}</span>
                            </small>
                        </label>
                    </div>
                )}
            </div>
        </div>
    );
}

export function ProfileSubAgentAccessSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const callableSubAgent = isCallableAsSubAgent(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="subagent-access">
            <div className="ttas-section-title">
                <i className="fa-solid fa-people-arrows"></i>
                <h4>{tr('subAgentAccess')}</h4>
            </div>
            <div className="ttas-delegation-panel">
                <label className="ttas-switch-row">
                    <input
                        type="checkbox"
                        checked={callableSubAgent}
                        disabled={builtin}
                        onChange={(event) => controller.setCallableAsSubAgent(event.target.checked)}
                    />
                    <span>
                        <strong>{tr('callableSubAgentToggle')}</strong>
                        <small>{tr('callableSubAgentHint')}</small>
                    </span>
                </label>
                {callableSubAgent && (
                    <div className="ttas-form-grid ttas-delegation-controls">
                        <label className="ttas-field ttas-span-2">
                            <span>{tr('agentFacingDescription')}</span>
                            <textarea
                                className="text_pole textarea_compact"
                                rows={4}
                                value={draft.delegation.descriptionForAgents ?? ''}
                                disabled={builtin}
                                placeholder={draft.description ?? ''}
                                onChange={(event) => controller.setDelegationDescription(event.target.value)}
                            ></textarea>
                            <small className="ttas-field-hint">
                                <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                                <span>{tr('agentFacingDescriptionHint')}</span>
                            </small>
                        </label>
                        <label className="ttas-field ttas-span-2">
                            <span>{tr('allowedCallers')}</span>
                            <input
                                className="text_pole"
                                value={draft.delegation.allowedCallersCsv ?? ''}
                                disabled={builtin}
                                onChange={(event) => controller.setAllowedCallersCsv(event.target.value)}
                            />
                            <small className="ttas-field-hint">
                                <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                                <span>{tr('allowedCallersHint')}</span>
                            </small>
                        </label>
                    </div>
                )}
            </div>
        </div>
    );
}

export function ProfileRunSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const presentationLocked = isSubAgentOnly(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="run">
            <div className="ttas-section-title">
                <i className="fa-solid fa-gauge-high"></i>
                <h4>{tr('runPolicy')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-switch-row ttas-span-2">
                    <input
                        type="checkbox"
                        checked={draft.run.stream}
                        disabled={builtin}
                        onChange={(event) => controller.setRunStream(event.target.checked)}
                    />
                    <span>
                        <strong>{tr('streaming')}</strong>
                        <small>{tr('streamingHint')}</small>
                    </span>
                </label>
                <label className="ttas-field">
                    <span>{tr('presentation')}</span>
                    <select
                        value={draft.run.presentation}
                        disabled={builtin || presentationLocked}
                        onChange={(event) => controller.setRunPresentation(event.target.value)}
                    >
                        <option value="foreground">{tr('foreground')}</option>
                        <option value="background">{tr('background')}</option>
                    </select>
                </label>
                <label className="ttas-field">
                    <span>{tr('planMode')}</span>
                    <select
                        value={draft.plan.mode}
                        disabled={builtin}
                        onChange={(event) => controller.setPlanMode(event.target.value)}
                    >
                        <option value="none">{tr('none')}</option>
                    </select>
                </label>
                <label className="ttas-field">
                    <span>{tr('maxRounds')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        value={draft.tools.maxRounds}
                        disabled={builtin}
                        onChange={(event) => controller.setToolsLimitField('maxRounds', parseNumberInput(event.target.value))}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('maxToolCalls')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        value={draft.tools.maxCallsPerRun}
                        disabled={builtin}
                        onChange={(event) => controller.setToolsLimitField('maxCallsPerRun', parseNumberInput(event.target.value))}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('mcpResultInlineCharLimit')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        step="1000"
                        value={draft.tools.mcpResultInlineCharLimit}
                        disabled={builtin}
                        onChange={(event) => controller.setToolsLimitField('mcpResultInlineCharLimit', parseNumberInput(event.target.value))}
                    />
                    <small className="ttas-field-hint">
                        <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                        <span>{tr('mcpResultInlineCharLimitHint')}</span>
                    </small>
                </label>
                <label className="ttas-field">
                    <span>{tr('modelRetries')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="0"
                        value={draft.run.modelRetry.maxRetries}
                        disabled={builtin}
                        onChange={(event) => controller.setModelRetryField('maxRetries', parseNumberInput(event.target.value))}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('retryIntervalMs')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        value={draft.run.modelRetry.intervalMs}
                        disabled={builtin}
                        onChange={(event) => controller.setModelRetryField('intervalMs', parseNumberInput(event.target.value))}
                    />
                </label>
            </div>
        </div>
    );
}

export function ProfileContextSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="context">
            <div className="ttas-section-title">
                <i className="fa-solid fa-layer-group"></i>
                <h4>{tr('initialContext')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-field">
                    <span>{tr('initialChatHistoryMessages')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        step="1"
                        value={draft.context.initialChatHistoryMessages}
                        disabled={builtin}
                        onChange={(event) => controller.setContextHistoryMessages(parseNumberInput(event.target.value))}
                    />
                    <small className="ttas-field-hint">
                        <i className="fa-solid fa-circle-info" aria-hidden="true"></i>
                        <span>{tr('initialChatHistoryMessagesHint')}</span>
                    </small>
                </label>
                <label className="checkbox_label ttas-field">
                    <span>{tr('includeActivatedWorldInfo')}</span>
                    <input
                        type="checkbox"
                        checked={draft.context.includeActivatedWorldInfo}
                        disabled={builtin}
                        onChange={(event) => controller.setContextIncludeWorldInfo(event.target.checked)}
                    />
                </label>
            </div>
        </div>
    );
}

export function ProfilePromptSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const isRuntimeStateCurrent = isProfileRuntimeStateCurrent(draft, snapshot.profileRuntimeStateJson);
    return (
        <div className="ttas-section" data-ttas-profile-section="prompt">
            <div className="ttas-section-title">
                <i className="fa-solid fa-terminal"></i>
                <h4>{tr('prompt')}</h4>
            </div>
            <label className="ttas-field">
                <span>{tr('agentSystemPrompt')}</span>
                <textarea
                    className="text_pole textarea_compact ttas-system-prompt-textarea"
                    rows={12}
                    value={agentSystemPromptEditorValue(draft, snapshot.resolvedAgentSystemPrompt)}
                    placeholder={agentSystemPromptPlaceholder(draft, snapshot.resolvedAgentSystemPrompt, isRuntimeStateCurrent)}
                    disabled={builtin}
                    onChange={(event) => controller.setAgentSystemPrompt(event.target.value)}
                ></textarea>
            </label>
        </div>
    );
}
