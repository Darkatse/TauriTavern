import { findModelTargetForBinding } from '../../../tauritavern/agent/model-target-llm-connection.js';
import type { ModelTarget } from './host';
import { ComposerMenu, MenuRadio } from './ComposerMenu';
import { Icon } from './components';
import { tr } from './i18n';

export type ModelChoice =
    | { state: 'ready'; target: ModelTarget; label: string }
    | { state: 'missing' | 'unset'; target: null; label: string };

export function modelChoice(models: readonly ModelTarget[], binding: TauriTavernAgentProfileDefinition['model']): ModelChoice {
    if (binding.mode !== 'connectionRef') return { state: 'unset', target: null, label: tr('chooseModel') };
    const target = findModelTargetForBinding(models, binding);
    if (target) return { state: 'ready', target, label: targetName(target) };
    return { state: 'missing', target: null, label: binding.modelId || tr('chooseModel') };
}
function targetName(target: ModelTarget): string {
    return target.name || target.model || target.id;
}

export function ModelMenu({ models, choice, open, disabled, onOpenChange, onSelect, onManage }: {
    models: readonly ModelTarget[]; choice: ModelChoice; open: boolean; disabled: boolean;
    onOpenChange: (open: boolean) => void; onSelect: (target: ModelTarget) => void; onManage: () => void;
}) {
    return <ComposerMenu className="ttia-model" state={choice.state} label={tr('modelMenu', { name: choice.label })}
        title={choice.state === 'missing' ? tr('bindingMissing') : undefined} name={tr('model')}
        open={open} disabled={disabled} onOpenChange={onOpenChange} trigger={<>
            <span className="ttia-chip-icon" aria-hidden="true"><Icon name="circle-nodes" /></span>
            <span className="ttia-chip-label" key={choice.label}>{choice.label}</span>
            <Icon name="chevron-down" />
        </>}>
        {choice.state === 'missing' && <div className="ttia-menu-item is-missing" role="menuitemradio" aria-checked="true" aria-disabled="true" title={tr('bindingMissing')}>
            <span className="ttia-menu-label">{choice.label}</span><span className="ttia-menu-detail">{tr('modelUnavailable')}</span><Icon name="check" />
        </div>}
        {models.map(target => <MenuRadio key={target.id} checked={target.id === choice.target?.id} label={targetName(target)}
            title={targetName(target)} onSelect={() => onSelect(target)} />)}
        {models.length === 0 && <div className="ttia-model-empty" role="menuitem" aria-disabled="true">
            <p>{tr('noModels')}</p><span>{tr('noModelsNote')}</span>
        </div>}
        <div className="ttia-menu-rule" role="separator" />
        <button type="button" tabIndex={-1} className="ttia-menu-item ttia-model-manage" role="menuitem" onClick={onManage}>
            <span className="ttia-menu-label">{tr('connections')}</span><Icon name="arrow-up-right-from-square" />
        </button>
    </ComposerMenu>;
}
