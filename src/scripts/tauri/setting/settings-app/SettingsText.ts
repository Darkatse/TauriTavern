/**
 * Pure display-text builders for the Settings view. No state, no React, no
 * host access — the React components consume these as plain functions.
 */

import { formatBytes } from '../format-bytes.js';
import type {
    ChatBackupStorageStats,
    DynamicThemeDraft,
    SettingsDataRootState,
    SettingsOption,
    SettingsTranslate,
} from './SettingsContract';

export function translateOptions(
    options: { value: string; labelKey: string }[],
    tr: SettingsTranslate,
): SettingsOption[] {
    return options.map(option => ({ value: option.value, label: tr(option.labelKey) }));
}

export type ZstdCompressionHint = { summary: string; before: string; saved: string; after: string };

export function zstdCompressionHint(
    tr: SettingsTranslate,
    enabled: boolean,
    stats: ChatBackupStorageStats | null,
): ZstdCompressionHint {
    const summary = tr('Saves substantial space, but SillyTavern cannot read this format.');
    const originalBytes = stats?.originalBytes ?? 0;
    const storedBytes = stats?.storedBytes ?? 0;
    if (!enabled || originalBytes <= storedBytes) {
        return { summary, before: '', saved: '', after: '' };
    }

    const ratio = Math.round(storedBytes / originalBytes * 100);
    const [before = '', after = ''] = tr(
        'Compressed backups currently use about {ratio}% of their original size and have saved about {saved}.',
    )
        .replace('{ratio}', String(ratio))
        .split('{saved}');
    return { summary, before, saved: formatBytes(originalBytes - storedBytes), after };
}

export function dataRootSummary(dataRoot: SettingsDataRootState | null, tr: SettingsTranslate): string {
    if (!dataRoot) {
        return '';
    }
    if (dataRoot.migrationError) {
        return tr('Data directory migration failed:');
    }
    if (dataRoot.migrationPending) {
        return tr('Data directory migration is pending.');
    }
    return dataRoot.currentDataRoot;
}

/** Disclosure meta shows live state instead of a "click to expand" hint. */
export function requestProxySummary(
    proxy: { enabled: boolean; url: string },
    tr: SettingsTranslate,
): string {
    if (!proxy.enabled) {
        return tr('Off');
    }
    return proxy.url || tr('Enabled');
}

export function dynamicAppearanceSummary(theme: DynamicThemeDraft, tr: SettingsTranslate): string {
    const themeSummary = theme.themeEnabled
        ? [theme.dayTheme, theme.nightTheme].filter(Boolean).join(' / ') || tr('Enabled')
        : tr('Off');
    return `${themeSummary} · ${tr(theme.wallpaperEnabled ? 'Enabled' : 'Off')}`;
}

export function dataRootStatus(dataRoot: SettingsDataRootState | null, tr: SettingsTranslate): string {
    if (!dataRoot) {
        return '';
    }
    if (dataRoot.migrationError) {
        return `${tr('Data directory migration failed:')} ${dataRoot.migrationError}`;
    }
    if (dataRoot.migrationPending) {
        const configuredLine = dataRoot.configuredDataRoot
            ? `${tr('Configured data directory:')} ${dataRoot.configuredDataRoot}`
            : '';
        const pendingLine = tr('Data directory migration is pending.');
        return configuredLine ? `${configuredLine}\n${pendingLine}` : pendingLine;
    }
    if (dataRoot.configuredDataRoot && dataRoot.configuredDataRoot !== dataRoot.currentDataRoot) {
        return `${tr('Configured data directory:')} ${dataRoot.configuredDataRoot}`;
    }
    return '';
}
