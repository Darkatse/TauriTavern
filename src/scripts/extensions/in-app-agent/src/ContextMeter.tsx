import { ComposerMenu, MenuAction } from './ComposerMenu';
import { tr } from './i18n';
import { contextUsage } from './session-state';

const RING = 2 * Math.PI * 5.5;
const COMPACT: Intl.NumberFormatOptions = { notation: 'compact', maximumFractionDigits: 1 };

// The share is of the compaction point: a full ring is where the runtime compacts by itself.
export function ContextMeter({ events, open, canCompact, onOpenChange, onCompact }: {
    events: TauriTavernAgentRunEvent[]; open: boolean; canCompact: boolean;
    onOpenChange: (open: boolean) => void; onCompact: () => void;
}) {
    const usage = contextUsage(events);
    if (!usage) return null;
    const locale = document.documentElement.lang || undefined;
    const share = Math.min(usage.tokens / usage.point, 1);
    const used = tr('contextTokens', { tokens: usage.tokens.toLocaleString(locale, COMPACT), point: usage.point.toLocaleString(locale, COMPACT) });
    return <ComposerMenu className="ttia-context" label={tr('context')} title={used} name={tr('context')}
        open={open} disabled={false} onOpenChange={onOpenChange} trigger={<>
            <svg className="ttia-context-ring" viewBox="0 0 14 14" aria-hidden="true">
                <circle cx="7" cy="7" r="5.5" />
                <circle cx="7" cy="7" r="5.5" strokeDasharray={`${share * RING} ${RING}`} />
            </svg>
            <span className="ttia-chip-label">{share.toLocaleString(locale, { style: 'percent', maximumFractionDigits: 0 })}</span>
        </>}>
        <MenuAction label={tr('compactNow')} detail={used} disabled={!canCompact} onSelect={onCompact} />
    </ComposerMenu>;
}
