import { useLayoutEffect, useRef, useState } from 'react';

import type {
    DynamicThemeDraft,
    SettingsActions,
    SettingsBackgroundOption,
    SettingsOption,
    SettingsTranslate,
} from './SettingsContract';
import { SelectField, SettingRow, ToggleSwitch, WallpaperField } from './SettingsComponents';
import { dynamicAppearanceSummary } from './SettingsText';

/** Unknown but persisted values stay visible instead of being silently reset. */
function themeOptionsWithStored(options: SettingsOption[], storedValue: string): SettingsOption[] {
    const normalized = storedValue.trim();
    if (!normalized || options.some(option => option.value === normalized)) {
        return options;
    }
    return [...options, { value: normalized, label: normalized }];
}

/** Wallpaper filenames intentionally keep their whitespace (no trim). */
function backgroundOptionsWithStored(
    options: SettingsBackgroundOption[],
    storedValue: string,
): SettingsBackgroundOption[] {
    if (!storedValue || options.some(option => option.value === storedValue)) {
        return options;
    }
    return [...options, { value: storedValue, label: storedValue, thumbnailUrl: '', isAnimated: false }];
}

function backgroundOption(options: SettingsBackgroundOption[], storedValue: string): SettingsBackgroundOption | null {
    return backgroundOptionsWithStored(options, storedValue).find(option => option.value === storedValue) ?? null;
}

/** Enable-time theme fallback: only fills themes the user left empty. */
function withThemeDefaults(draft: DynamicThemeDraft, fallbackTheme: string): DynamicThemeDraft {
    return {
        ...draft,
        themeEnabled: true,
        dayTheme: draft.dayTheme || fallbackTheme,
        nightTheme: draft.nightTheme || fallbackTheme,
    };
}

/** Enable-time wallpaper fallback: only fills wallpapers the user left empty. */
function withWallpaperDefaults(draft: DynamicThemeDraft, fallbackWallpaper: string): DynamicThemeDraft {
    return {
        ...draft,
        wallpaperEnabled: true,
        dayWallpaper: draft.dayWallpaper || fallbackWallpaper,
        nightWallpaper: draft.nightWallpaper || fallbackWallpaper,
    };
}

type SettingsAppearanceSectionProps = {
    theme: DynamicThemeDraft;
    open: boolean;
    themeOptions: SettingsOption[];
    backgroundOptions: SettingsBackgroundOption[];
    currentBackground: string;
    chooseWallpaper: SettingsActions['chooseWallpaper'];
    tr: SettingsTranslate;
    onOpenChange: (open: boolean) => void;
    onPatch: (patch: Partial<DynamicThemeDraft>) => void;
    onShowHelp: (topic: string) => void;
};

/** The "Dynamic Theme & Wallpaper" disclosure of the Misc section. */
export function SettingsAppearanceSection({
    theme,
    open,
    themeOptions,
    backgroundOptions,
    currentBackground,
    chooseWallpaper,
    tr,
    onOpenChange,
    onPatch,
    onShowHelp,
}: SettingsAppearanceSectionProps) {
    const [focusTick, setFocusTick] = useState(0);
    const dayThemeRef = useRef<HTMLSelectElement>(null);

    // One-shot focus intent: enabling theme switching expands the disclosure
    // and focuses the Day Theme select in the same commit, without timeouts.
    // The tick counter makes repeated enable toggles re-fire the effect.
    useLayoutEffect(() => {
        if (focusTick === 0) {
            return;
        }
        dayThemeRef.current?.focus();
    }, [focusTick]);

    const fallbackTheme = themeOptions[0]?.value || '';
    const fallbackWallpaper = backgroundOptions.some(option => option.value === currentBackground)
        ? currentBackground
        : backgroundOptions[0]?.value || '';

    function setThemeSwitchingEnabled(enabled: boolean): void {
        onPatch(enabled ? withThemeDefaults(theme, fallbackTheme) : { themeEnabled: false });
        if (!enabled) {
            return;
        }
        onOpenChange(true);
        setFocusTick(tick => tick + 1);
    }

    function setWallpaperSwitchingEnabled(enabled: boolean): void {
        onPatch(enabled ? withWallpaperDefaults(theme, fallbackWallpaper) : { wallpaperEnabled: false });
        if (enabled) {
            onOpenChange(true);
        }
    }

    async function chooseWallpaperFor(targetKey: 'dayWallpaper' | 'nightWallpaper'): Promise<void> {
        const selected = await chooseWallpaper({ currentValue: theme[targetKey] });
        if (!selected) {
            return;
        }
        onPatch({ [targetKey]: selected });
    }

    return (
        <details
            className="tt-settings-disclosure"
            open={open}
            onToggle={event => onOpenChange(event.currentTarget.open)}
        >
            <summary>
                <span>{tr('Dynamic Theme & Wallpaper')}</span>
                <span className="tt-settings-summary-meta">
                    <small>{dynamicAppearanceSummary(theme, tr)}</small>
                    <i className="fa-solid fa-chevron-down" aria-hidden="true"></i>
                </span>
            </summary>
            <div className="tt-settings-disclosure-body">
                <SettingRow
                    label={tr('Enable Theme Switching')}
                    helpTopic="dynamicTheme"
                    helpTitle={tr('Learn more')}
                    onHelp={onShowHelp}
                >
                    <ToggleSwitch
                        checked={theme.themeEnabled}
                        ariaLabel={tr('Enable Theme Switching')}
                        onChange={setThemeSwitchingEnabled}
                    />
                </SettingRow>
                <SettingRow label={tr('Day Theme')}>
                    <SelectField
                        ref={dayThemeRef}
                        value={theme.dayTheme}
                        options={themeOptionsWithStored(themeOptions, theme.dayTheme)}
                        disabled={!theme.themeEnabled}
                        ariaLabel={tr('Day Theme')}
                        onChange={value => onPatch({ dayTheme: value })}
                    />
                </SettingRow>
                <SettingRow label={tr('Night Theme')}>
                    <SelectField
                        value={theme.nightTheme}
                        options={themeOptionsWithStored(themeOptions, theme.nightTheme)}
                        disabled={!theme.themeEnabled}
                        ariaLabel={tr('Night Theme')}
                        onChange={value => onPatch({ nightTheme: value })}
                    />
                </SettingRow>
                <SettingRow label={tr('Enable Wallpaper Switching')}>
                    <ToggleSwitch
                        checked={theme.wallpaperEnabled}
                        ariaLabel={tr('Enable Wallpaper Switching')}
                        onChange={setWallpaperSwitchingEnabled}
                    />
                </SettingRow>
                <SettingRow label={tr('Day Wallpaper')}>
                    <WallpaperField
                        option={backgroundOption(backgroundOptions, theme.dayWallpaper)}
                        value={theme.dayWallpaper}
                        placeholder={tr('Choose Wallpaper')}
                        disabled={!theme.wallpaperEnabled}
                        onChoose={() => void chooseWallpaperFor('dayWallpaper')}
                    />
                </SettingRow>
                <SettingRow label={tr('Night Wallpaper')}>
                    <WallpaperField
                        option={backgroundOption(backgroundOptions, theme.nightWallpaper)}
                        value={theme.nightWallpaper}
                        placeholder={tr('Choose Wallpaper')}
                        disabled={!theme.wallpaperEnabled}
                        onChoose={() => void chooseWallpaperFor('nightWallpaper')}
                    />
                </SettingRow>
                <small className="tt-settings-section-note">{tr('Dynamic Theme & Wallpaper hint')}</small>
            </div>
        </details>
    );
}
