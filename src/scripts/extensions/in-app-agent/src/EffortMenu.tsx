import { ComposerMenu, MenuRadio } from './ComposerMenu';
import { Icon } from './components';
import { tr, type MessageKey } from './i18n';

type Profile = TauriTavernAgentProfileDefinition;
type Effort = TauriTavernReasoningEffort;

const LABELS: Record<Effort, MessageKey> = {
    auto: 'effortAuto', min: 'effortMin', low: 'effortLow', medium: 'effortMedium', high: 'effortHigh', xhigh: 'effortXhigh', max: 'effortMax',
};
const LEVELS = ['max', 'xhigh', 'high', 'medium', 'low', 'min'] as const satisfies readonly Effort[];

function effortLabel(value: string): string {
    return value in LABELS ? tr(LABELS[value as Effort]) : value;
}

export function withReasoningEffort(profile: Profile, effort: Effort | undefined): Profile {
    const preset = { ...profile.preset };
    if (effort) preset.reasoningEffort = effort;
    else delete preset.reasoningEffort;
    return { ...profile, preset };
}

// The chip shows the configured level; prompt assembly maps it for the source and model.
export function EffortMenu({ effort, preset, presetEffort, open, disabled, onOpenChange, onSelect }: {
    effort: Effort | undefined; preset: string; presetEffort: string | null; open: boolean; disabled: boolean;
    onOpenChange: (open: boolean) => void; onSelect: (effort: Effort | undefined) => void;
}) {
    const configured = effort ?? presetEffort;
    const label = configured ? effortLabel(configured) : tr('effortPreset');
    const summary = tr(!effort && configured ? 'effortMenuPreset' : 'effortMenu', { name: label });
    return <ComposerMenu className="ttia-effort" label={summary} title={summary} name={tr('reasoningEffort')}
        open={open} disabled={disabled} onOpenChange={onOpenChange} trigger={<>
            <span className="ttia-chip-icon" aria-hidden="true"><Icon name="lightbulb" /></span>
            <span className="ttia-chip-label" key={label}>{label}</span>
        </>}>
        {LEVELS.map(level => <MenuRadio key={level} checked={effort === level} label={effortLabel(level)} onSelect={() => onSelect(level)} />)}
        <div className="ttia-menu-rule" role="separator" />
        <MenuRadio checked={effort === 'auto'} label={effortLabel('auto')} title={tr('effortAutoNote')} onSelect={() => onSelect('auto')} />
        <MenuRadio checked={!effort} label={tr('effortPreset')} detail={presetEffort ? effortLabel(presetEffort) : undefined}
            title={tr('effortPresetNote', { preset })} onSelect={() => onSelect(undefined)} />
    </ComposerMenu>;
}
