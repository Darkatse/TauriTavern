import { useMemo } from 'react';
import { ComposerMenu, MenuAction } from './ComposerMenu';
import { tr } from './i18n';
import { contextUsage } from './session-state';

const RING = 2 * Math.PI * 5.5;

// The share is of the compaction point: a full ring is where the runtime compacts by itself.
export function ContextMeter({ events, open, canCompact, onOpenChange, onCompact }: {
    events: TauriTavernAgentRunEvent[]; open: boolean; canCompact: boolean;
    onOpenChange: (open: boolean) => void; onCompact: () => void;
}) {
    const locale = document.documentElement.lang || undefined;
    const compact = useMemo(() => new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }), [locale]);
    const percent = useMemo(() => new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }), [locale]);
    const usage = contextUsage(events);
    if (!usage) return null;
    const share = Math.min(usage.tokens / usage.point, 1);
    const used = tr('contextTokens', { tokens: compact.format(usage.tokens), point: compact.format(usage.point) });
    return <ComposerMenu className="ttia-context" label={tr('context')} title={used} name={tr('context')}
        open={open} disabled={false} onOpenChange={onOpenChange} trigger={<>
            <svg className="ttia-context-ring" viewBox="0 0 14 14" aria-hidden="true">
                <circle cx="7" cy="7" r="5.5" />
                <circle cx="7" cy="7" r="5.5" strokeDasharray={`${share * RING} ${RING}`} />
            </svg>
            <span className="ttia-chip-label">{percent.format(share)}</span>
        </>}>
        <MenuAction label={tr('compactNow')} detail={used} disabled={!canCompact} onSelect={onCompact} />
    </ComposerMenu>;
}
