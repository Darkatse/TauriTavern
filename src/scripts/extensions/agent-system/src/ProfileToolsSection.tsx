import type { AgentSystemPanelController } from './AgentSystemPanelController';
import {
    isBuiltinProfile,
    type AgentSystemPanelSnapshot,
    type Tr,
} from './AgentSystemPanelContract';
import {
    enabledToolCount,
    getToolDescriptionOverride,
    getToolPropertyDescriptionOverride,
    selectedToolProperties,
    toolBadges,
    toolGroupsWithTools,
    toolHasDescriptionOverride,
    toolItemsById,
    toolReferenceLabel,
    toolSource,
    toolTitle,
} from './AgentSystemPanelView';

export type ProfileToolsSectionProps = {
    snapshot: AgentSystemPanelSnapshot;
    controller: AgentSystemPanelController;
    tr: Tr;
};

export function ProfileToolsSection({ snapshot, controller, tr }: ProfileToolsSectionProps) {
    const { draft, toolIds, selectedToolId } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const groups = toolGroupsWithTools(toolIds);
    const itemsById = toolItemsById(snapshot.toolItems);
    const selectedToolItem = itemsById[selectedToolId] ?? null;
    const selectedToolEnabled = Array.isArray(draft.tools?.allow) && draft.tools.allow.includes(selectedToolId);
    const properties = selectedToolProperties(selectedToolItem, tr);

    return (
        <div className="ttas-section" data-ttas-profile-section="tools">
            <div className="ttas-section-title">
                <i className="fa-solid fa-screwdriver-wrench"></i>
                <h4>{tr('capabilityMatrix')}</h4>
            </div>
            <div className="ttas-tool-workbench">
                <div className="ttas-tool-groups">
                    {groups.map((group) => (
                        <div key={group.id} className="ttas-tool-group">
                            <header>
                                <i className={`fa-solid ${group.icon}`}></i>
                                <strong>{tr(group.labelKey)}</strong>
                                <span>{enabledToolCount(draft, group.tools)}/{group.tools.length}</span>
                            </header>
                            <div className="ttas-tool-list">
                                {group.tools.map((tool) => {
                                    const toolItem = itemsById[tool];
                                    const rowClass = [
                                        'ttas-tool-row',
                                        selectedToolId === tool ? 'active' : '',
                                        draft.tools.allow.includes(tool) ? 'enabled' : '',
                                        toolHasDescriptionOverride(draft, tool) ? 'customized' : '',
                                    ].filter(Boolean).join(' ');
                                    return (
                                        <div key={tool} className={rowClass}>
                                            <input
                                                type="checkbox"
                                                checked={draft.tools.allow.includes(tool)}
                                                disabled={builtin}
                                                onChange={(event) => void controller.toggleToolAllowed(tool, event.target.checked)}
                                            />
                                            <button type="button" className="ttas-tool-select" onClick={() => controller.selectTool(tool)}>
                                                <strong>{toolTitle(toolItem, tool)}</strong>
                                                <span>{toolReferenceLabel(toolItem, tool)}</span>
                                            </button>
                                            {toolHasDescriptionOverride(draft, tool) && (
                                                <i className="fa-solid fa-pen-nib ttas-tool-custom-marker" title={tr('customizedTool')}></i>
                                            )}
                                        </div>
                                    );
                                })}
                            </div>
                        </div>
                    ))}
                </div>

                {selectedToolItem && (
                    <aside className="ttas-tool-editor-panel">
                        <header className="ttas-tool-editor-header">
                            <div>
                                <div className="ttas-eyebrow">{toolReferenceLabel(selectedToolItem, selectedToolId)}</div>
                                <h5>{selectedToolItem.title}</h5>
                            </div>
                            <button
                                type="button"
                                className="menu_button menu_button_icon"
                                disabled={builtin || !toolHasDescriptionOverride(draft, selectedToolId)}
                                onClick={() => controller.resetToolDescriptionOverride(selectedToolId)}
                            >
                                <i className="fa-solid fa-rotate-left"></i>
                                <span>{tr('reset')}</span>
                            </button>
                        </header>

                        <div className="ttas-tool-badge-row">
                            {toolSource(selectedToolItem, tr) && <span>{toolSource(selectedToolItem, tr)}</span>}
                            {toolBadges(draft, selectedToolItem, selectedToolId, tr).map((badge) => (
                                <span key={badge.key} className={`ttas-tool-badge-${badge.key}`}>{badge.label}</span>
                            ))}
                            {!selectedToolEnabled && <span className="ttas-tool-badge-disabled">{tr('disabledTool')}</span>}
                        </div>

                        <div className="ttas-tool-default-description">
                            <span>{tr('defaultDescription')}</span>
                            <p>{selectedToolItem.description}</p>
                        </div>

                        <label className="ttas-field">
                            <span>{tr('customToolDescription')}</span>
                            <textarea
                                className="text_pole textarea_compact ttas-tool-description-textarea"
                                rows={5}
                                value={getToolDescriptionOverride(draft, selectedToolId)}
                                placeholder={selectedToolItem.description}
                                disabled={builtin || !selectedToolEnabled}
                                onChange={(event) => controller.setToolDescriptionOverride(selectedToolId, event.target.value)}
                            ></textarea>
                        </label>

                        <div className="ttas-tool-property-list">
                            <div className="ttas-tool-property-title">
                                <i className="fa-solid fa-sliders"></i>
                                <strong>{tr('toolParameters')}</strong>
                            </div>
                            {properties.length === 0 && <div className="ttas-empty">{tr('noToolParameters')}</div>}
                            {properties.map((property) => (
                                <div key={property.name} className="ttas-tool-property-row">
                                    <div className="ttas-tool-property-meta">
                                        <code>{property.name}</code>
                                        <span>{property.type}</span>
                                        {property.required && <em>{tr('required')}</em>}
                                    </div>
                                    {property.description && <p>{property.description}</p>}
                                    <div className="ttas-tool-property-edit">
                                        <textarea
                                            className="text_pole textarea_compact"
                                            rows={3}
                                            value={getToolPropertyDescriptionOverride(draft, selectedToolId, property.name)}
                                            placeholder={property.description}
                                            disabled={builtin || !selectedToolEnabled}
                                            onChange={(event) => controller.setToolPropertyDescriptionOverride(selectedToolId, property.name, event.target.value)}
                                        ></textarea>
                                        <button
                                            type="button"
                                            className="menu_button menu_button_icon"
                                            disabled={builtin || !getToolPropertyDescriptionOverride(draft, selectedToolId, property.name)}
                                            onClick={() => controller.resetToolPropertyDescriptionOverride(selectedToolId, property.name)}
                                        >
                                            <i className="fa-solid fa-rotate-left"></i>
                                            <span>{tr('reset')}</span>
                                        </button>
                                    </div>
                                </div>
                            ))}
                        </div>
                    </aside>
                )}
            </div>
        </div>
    );
}
