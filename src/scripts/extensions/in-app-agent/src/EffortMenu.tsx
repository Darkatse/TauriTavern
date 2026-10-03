import { ComposerMenu, MenuRadio } from './ComposerMenu';
import { Icon } from './components';
import { tr, type MessageKey } from './i18n';

type Profile = TauriTavernAgentProfileDefinition;
type Effort = TauriTavernReasoningEffort;

const LABELS: Record<Effort, MessageKey> = {
    auto: 'effortAuto', none: 'effortNone', min: 'effortMin', minimal: 'effortMinimal', low: 'effortLow',
    medium: 'effortMedium', high: 'effortHigh', xhigh: 'effortXhigh', max: 'effortMax',
};
const LEVELS = ['max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'min', 'none'] as const satisfies readonly Effort[];

function effortLabel(value: string): string {
    return value in LABELS ? tr(LABELS[value as Effort]) : value;
}

export function withReasoningEffort(profile: Profile, effort: Effort | undefined): Profile {
    const preset = { ...profile.preset };
    if (effort) preset.reasoningEffort = effort;
    else delete preset.reasoningEffort;
    return { ...profile, preset };
}

// The chip shows what is sent: `resolve` maps a stored level to the target's API format, or to
// 'auto' when the target can't send it (an override then counts as unset).
export function EffortMenu({ effort: storedEffort, options, resolve, preset, presetEffort: storedPresetEffort, open, disabled, onOpenChange, onSelect }: {
    effort: Effort | undefined; options: readonly string[]; resolve: (value: string) => string; preset: string; presetEffort: string | null; open: boolean; disabled: boolean;
    onOpenChange: (open: boolean) => void; onSelect: (effort: Effort | undefined) => void;
}) {
    const resolvedEffort = storedEffort && resolve(storedEffort);
    const effort = resolvedEffort && (storedEffort === 'auto' || resolvedEffort !== 'auto') ? resolvedEffort as Effort : undefined;
    const presetEffort = storedPresetEffort && resolve(storedPresetEffort);
    const configured = effort ?? presetEffort;
    const label = configured ? effortLabel(configured) : tr('effortPreset');
    const summary = tr(!effort && configured ? 'effortMenuPreset' : 'effortMenu', { name: label });
    return <ComposerMenu className="ttia-effort" label={summary} title={summary} name={tr('reasoningEffort')}
        open={open} disabled={disabled} onOpenChange={onOpenChange} trigger={<>
            <span className="ttia-chip-icon" aria-hidden="true"><Icon name="lightbulb" /></span>
            <span className="ttia-chip-label" key={label}>{label}</span>
        </>}>
        {LEVELS.filter(level => options.includes(level)).map(level =>
            <MenuRadio key={level} checked={effort === level} label={effortLabel(level)} onSelect={() => onSelect(level)} />)}
        <div className="ttia-menu-rule" role="separator" />
        <MenuRadio checked={effort === 'auto'} label={effortLabel('auto')} title={tr('effortAutoNote')} onSelect={() => onSelect('auto')} />
        <MenuRadio checked={!effort} label={tr('effortPreset')} detail={presetEffort ? effortLabel(presetEffort) : undefined}
            title={tr('effortPresetNote', { preset })} onSelect={() => onSelect(undefined)} />
    </ComposerMenu>;
}
