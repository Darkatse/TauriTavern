import { WORKSPACE_ROOTS } from './constants';
import type { AgentSystemPanelController } from './AgentSystemPanelController';
import {
    isBuiltinProfile,
    parseNumberInput,
    workspaceRootIcon,
    type AgentSystemPanelSnapshot,
    type Tr,
} from './AgentSystemPanelContract';

export type ProfileSectionProps = {
    snapshot: AgentSystemPanelSnapshot;
    controller: AgentSystemPanelController;
    tr: Tr;
};

export function ProfileSkillsSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="skills">
            <div className="ttas-section-title">
                <i className="fa-solid fa-book"></i>
                <h4>{tr('skillAccess')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-field">
                    <span>{tr('visibleSkills')}</span>
                    <input
                        className="text_pole"
                        value={draft.skills.visibleCsv ?? ''}
                        disabled={builtin}
                        onChange={(event) => controller.setSkillsCsvField('visibleCsv', event.target.value)}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('deniedSkills')}</span>
                    <input
                        className="text_pole"
                        value={draft.skills.denyCsv ?? ''}
                        disabled={builtin}
                        onChange={(event) => controller.setSkillsCsvField('denyCsv', event.target.value)}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('maxCharsPerCall')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        value={draft.skills.maxReadCharsPerCall}
                        disabled={builtin}
                        onChange={(event) => controller.setSkillsLimitField('maxReadCharsPerCall', parseNumberInput(event.target.value))}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('maxCharsPerRun')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        min="1"
                        value={draft.skills.maxReadCharsPerRun}
                        disabled={builtin}
                        onChange={(event) => controller.setSkillsLimitField('maxReadCharsPerRun', parseNumberInput(event.target.value))}
                    />
                </label>
            </div>
        </div>
    );
}

export function ProfileWorkspaceSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    return (
        <div className="ttas-section" data-ttas-profile-section="workspace">
            <div className="ttas-section-title">
                <i className="fa-solid fa-folder-tree"></i>
                <h4>{tr('workspaceAccess')}</h4>
            </div>
            <div className="ttas-root-grid">
                {WORKSPACE_ROOTS.map((root) => (
                    <div key={root} className="ttas-root-row">
                        <div className="ttas-root-name">
                            <i className={`fa-solid ${workspaceRootIcon(root)}`}></i>
                            <strong>{root}</strong>
                        </div>
                        <label className="checkbox_label">
                            <input
                                type="checkbox"
                                checked={draft.workspace.visibleRoots.includes(root)}
                                disabled={builtin}
                                onChange={(event) => controller.setWorkspaceRootVisible(root, event.target.checked)}
                            />
                            <span>{tr('visible')}</span>
                        </label>
                        <label className="checkbox_label">
                            <input
                                type="checkbox"
                                checked={draft.workspace.writableRoots.includes(root)}
                                disabled={builtin || !draft.workspace.visibleRoots.includes(root)}
                                onChange={(event) => controller.setWorkspaceRootWritable(root, event.target.checked)}
                            />
                            <span>{tr('writable')}</span>
                        </label>
                    </div>
                ))}
            </div>
        </div>
    );
}

export function ProfileOutputSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const [artifact] = draft.output.artifacts;
    if (!artifact) {
        throw new Error('Agent profile is missing output.artifacts[0]');
    }
    return (
        <div className="ttas-section" data-ttas-profile-section="output">
            <div className="ttas-section-title">
                <i className="fa-solid fa-file-lines"></i>
                <h4>{tr('outputArtifact')}</h4>
            </div>
            <div className="ttas-form-grid">
                <label className="ttas-field">
                    <span>{tr('messageBodyPath')}</span>
                    <input
                        className="text_pole"
                        value={artifact.path}
                        disabled={builtin}
                        onChange={(event) => controller.setOutputArtifactField('path', event.target.value)}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('kind')}</span>
                    <input
                        className="text_pole"
                        value={artifact.kind}
                        disabled={builtin}
                        onChange={(event) => controller.setOutputArtifactField('kind', event.target.value)}
                    />
                </label>
            </div>
        </div>
    );
}

export function ProfileJsonSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft, draftJson } = snapshot;
    const builtin = isBuiltinProfile(draft);
    return (
        <div className="ttas-section ttas-json-section" data-ttas-profile-section="json">
            <div className="ttas-pane-header">
                <div className="ttas-section-title">
                    <i className="fa-solid fa-code"></i>
                    <h4>{tr('advancedJson')}</h4>
                </div>
                <div className="ttas-toolbar">
                    <button type="button" className="menu_button" onClick={() => controller.refreshDraftJson()}>{tr('refreshJson')}</button>
                    <button type="button" className="menu_button" disabled={builtin} onClick={() => controller.applyDraftJson()}>{tr('applyJson')}</button>
                </div>
            </div>
            <textarea
                className="text_pole ttas-json"
                value={draftJson}
                readOnly={builtin}
                onChange={(event) => controller.setDraftJson(event.target.value)}
            ></textarea>
        </div>
    );
}
