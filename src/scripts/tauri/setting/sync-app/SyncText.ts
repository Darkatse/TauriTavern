import { formatTimestampValue } from './format';
import {
    automationTargetValue,
    type SyncAutomationConfig,
    type SyncAutomationStatus,
    type SyncDatasetSelection,
    type SyncScopeDatasetCatalog,
    type SyncTarget,
    type SyncTranslate,
} from './SyncContract';

/**
 * Pure display projections of the Sync Main controller state. Kept separate
 * from the view so the rendering layer stays declarative and these stay
 * trivially testable.
 */

export function formatAutomationInterval(minutes: number, tr: SyncTranslate): string {
    const value = Number(minutes);
    if (!Number.isFinite(value)) {
        return `0 ${tr('minutes')}`;
    }
    if (value < 60) {
        return `${value} ${tr('minutes')}`;
    }

    const hours = value / 60;
    const hourText = Number.isInteger(hours) ? String(hours) : String(Math.round(hours * 10) / 10);
    return `${hourText} ${tr(hours === 1 ? 'hour' : 'hours')}`;
}

export function automationStatusText(status: SyncAutomationStatus, tr: SyncTranslate): string {
    if (status.running) {
        return tr('Uploading...');
    }
    if (status.lastError) {
        return `${tr('Last error')}: ${status.lastError}`;
    }
    if (status.nextRunAtMs) {
        return `${tr('Next run')}: ${formatTimestampValue(status.nextRunAtMs, tr)}`;
    }
    const lastSuccessAtMs = Number(status.lastSuccessAtMs || 0);
    const lastRequestAcceptedAtMs = Number(status.lastRequestAcceptedAtMs || 0);
    if (lastSuccessAtMs >= lastRequestAcceptedAtMs && lastSuccessAtMs > 0) {
        return `${tr('Last success')}: ${formatTimestampValue(lastSuccessAtMs, tr)}`;
    }
    if (lastRequestAcceptedAtMs > 0) {
        return `${tr('Last request accepted')}: ${formatTimestampValue(lastRequestAcceptedAtMs, tr)}`;
    }
    return tr('Idle');
}

export function formatShortTimestamp(ms: number | null | undefined, tr: SyncTranslate): string {
    if (!ms) {
        return tr('N/A');
    }
    const date = new Date(Number(ms));
    if (Number.isNaN(date.getTime())) {
        return tr('Invalid time');
    }
    // Pairing codes live for minutes: the time of day is the useful precision,
    // the date only matters once the expiry crosses midnight.
    const sameDay = date.toDateString() === new Date().toDateString();
    return sameDay
        ? date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        : date.toLocaleString([], { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

function defaultDatasetSelected(
    selection: SyncDatasetSelection | null,
    catalog: SyncScopeDatasetCatalog | null,
): boolean {
    const current = [...(selection?.dataset_ids || [])].sort();
    const defaults = [...(catalog?.defaultDatasetIds || [])].sort();
    return current.length > 0
        && current.length === defaults.length
        && current.every((id, index) => id === defaults[index]);
}

export function scopeSummaryText(
    selection: SyncDatasetSelection | null,
    catalog: SyncScopeDatasetCatalog | null,
    tr: SyncTranslate,
): string {
    const selected = selection?.dataset_ids.length || 0;
    const supported = catalog?.supportedDatasetIds.length || 0;
    if (!selected || !supported) {
        return tr('Sync content selection is unavailable');
    }
    return defaultDatasetSelected(selection, catalog)
        ? `${tr('Recommended default')} (${selected} / ${supported})`
        : `${selected} / ${supported} ${tr('datasets selected')}`;
}

export type AutomationTargetOption = {
    value: string;
    label: string;
    disabled: boolean;
};

export function automationTargetOptions(
    targets: SyncTarget[],
    config: SyncAutomationConfig,
): AutomationTargetOption[] {
    const automationMirror = config.syncMode === 'Mirror';
    return targets.map((target) => {
        if (target.type === 'lan') {
            return {
                value: automationTargetValue(target),
                label: `LAN · ${target.displayName}`,
                disabled: !target.lastKnownAddress,
            };
        }

        const canWrite = Boolean(target.permissions.write);
        const canMirror = Boolean(target.permissions.mirror_delete);
        return {
            value: automationTargetValue(target),
            label: `TT-Sync · ${target.displayName}`,
            disabled: !canWrite || (automationMirror && !canMirror),
        };
    });
}
