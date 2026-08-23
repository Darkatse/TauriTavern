import type { ReactNode, Ref } from 'react';

import type { SettingsBackgroundOption, SettingsOption } from './SettingsContract';

type SettingsSectionProps = {
    title: string;
    icon?: string;
    children: ReactNode;
};

export function SettingsSection({ title, icon = '', children }: SettingsSectionProps) {
    return (
        <section className="tt-settings-section">
            <div className="tt-settings-section-title">
                {icon && <i className={`fa-solid ${icon}`} aria-hidden="true"></i>}
                <b>{title}</b>
            </div>
            <div className="tt-settings-section-body">{children}</div>
        </section>
    );
}

type SettingRowProps = {
    label: string;
    hint?: ReactNode;
    helpTopic?: string;
    helpTitle?: string;
    onHelp?: (topic: string) => void;
    children: ReactNode;
};

export function SettingRow({ label, hint, helpTopic = '', helpTitle = '', onHelp, children }: SettingRowProps) {
    return (
        <div className="tt-settings-row">
            <div className="tt-settings-row-copy">
                <div className="tt-settings-label-line">
                    <span>{label}</span>
                    {helpTopic && (
                        <button
                            type="button"
                            className="tt-settings-icon-button"
                            title={helpTitle}
                            onClick={() => onHelp?.(helpTopic)}
                        >
                            <i className="fa-solid fa-circle-question" aria-hidden="true"></i>
                        </button>
                    )}
                </div>
            </div>
            <div className="tt-settings-control">{children}</div>
            {hint !== undefined && hint !== '' && <small className="tt-settings-hint">{hint}</small>}
        </div>
    );
}

type ToggleSwitchProps = {
    checked: boolean;
    disabled?: boolean;
    ariaLabel: string;
    onChange: (checked: boolean) => void;
};

export function ToggleSwitch({ checked, disabled = false, ariaLabel, onChange }: ToggleSwitchProps) {
    return (
        <label className="tt-settings-switch">
            <input
                type="checkbox"
                checked={checked}
                disabled={disabled}
                aria-label={ariaLabel}
                onChange={event => onChange(event.target.checked)}
            />
            <span aria-hidden="true"></span>
        </label>
    );
}

type SelectFieldProps = {
    value: string;
    options: SettingsOption[];
    disabled?: boolean;
    ariaLabel: string;
    onChange: (value: string) => void;
    ref?: Ref<HTMLSelectElement>;
};

export function SelectField({ value, options, disabled = false, ariaLabel, onChange, ref }: SelectFieldProps) {
    return (
        <select
            ref={ref}
            className="text_pole tt-settings-select"
            value={value}
            disabled={disabled}
            aria-label={ariaLabel}
            onChange={event => onChange(event.target.value)}
        >
            {options.map(option => (
                <option key={option.value} value={option.value}>{option.label}</option>
            ))}
        </select>
    );
}

type WallpaperFieldProps = {
    option: SettingsBackgroundOption | null;
    value: string;
    placeholder: string;
    disabled?: boolean;
    onChoose: () => void;
};

export function WallpaperField({ option, value, placeholder, disabled = false, onChoose }: WallpaperFieldProps) {
    const label = option?.label || value || placeholder;
    return (
        <button
            type="button"
            className="tt-settings-wallpaper-button"
            disabled={disabled}
            title={label}
            onClick={onChoose}
        >
            <span
                className="tt-settings-wallpaper-swatch"
                style={option?.thumbnailUrl ? { backgroundImage: `url("${option.thumbnailUrl}")` } : undefined}
            >
                {!option?.thumbnailUrl && <i className="fa-solid fa-image" aria-hidden="true"></i>}
            </span>
            <span className="tt-settings-wallpaper-label">{label}</span>
            <i className="fa-solid fa-chevron-right" aria-hidden="true"></i>
        </button>
    );
}

type ActionButtonProps = {
    label: string;
    icon?: string;
    title?: string;
    disabled?: boolean;
    onClick: () => void;
};

export function ActionButton({ label, icon = '', title = '', disabled = false, onClick }: ActionButtonProps) {
    return (
        <button
            type="button"
            className="menu_button menu_button_icon tt-settings-action-button"
            title={title || label}
            disabled={disabled}
            onClick={onClick}
        >
            {icon && <i className={`fa-solid ${icon}`} aria-hidden="true"></i>}
            <span>{label}</span>
        </button>
    );
}
