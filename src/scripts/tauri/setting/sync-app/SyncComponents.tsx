import type { ReactNode } from 'react';

import { formatTimestampValue } from './format';
import type { SyncTarget, SyncTranslate } from './SyncContract';

type SyncButtonProps = {
    label: string;
    icon?: string;
    title?: string;
    disabled?: boolean;
    danger?: boolean;
    iconOnly?: boolean;
    onClick: () => void;
};

export function SyncButton({
    label,
    icon = '',
    title = '',
    disabled = false,
    danger = false,
    iconOnly = false,
    onClick,
}: SyncButtonProps) {
    const text = title || label;
    return (
        <button
            type="button"
            className={`menu_button margin0 tt-sync-button${icon ? ' menu_button_icon' : ''}${danger ? ' red_button' : ''}`}
            title={text}
            aria-label={text}
            disabled={disabled}
            onClick={onClick}
        >
            {icon && <i className={`fa-solid ${icon}`} aria-hidden="true"></i>}
            {!iconOnly && <span>{label}</span>}
        </button>
    );
}

type SyncSectionProps = {
    title: string;
    actions?: ReactNode;
    children: ReactNode;
};

export function SyncSection({ title, actions, children }: SyncSectionProps) {
    return (
        <section className="tt-sync-section">
            <div className="tt-sync-section-header">
                <b>{title}</b>
                {actions}
            </div>
            {children}
        </section>
    );
}

type SyncSwitchProps = {
    checked: boolean;
    disabled?: boolean;
    label?: string;
    title?: string;
    onChange: (checked: boolean) => void;
};

export function SyncSwitch({
    checked,
    disabled = false,
    label = '',
    title = '',
    onChange,
}: SyncSwitchProps) {
    const text = title || label;
    return (
        <label className={`tt-sync-switch${disabled ? ' is-disabled' : ''}`} title={text}>
            <input
                type="checkbox"
                checked={checked}
                disabled={disabled}
                aria-label={text}
                onChange={event => onChange(event.target.checked)}
            />
            <span className="tt-sync-switch-track" aria-hidden="true"></span>
            {label && <span className="tt-sync-switch-label">{label}</span>}
        </label>
    );
}

type SyncTargetRowProps = {
    target: SyncTarget;
    running: boolean;
    tr: SyncTranslate;
    disabled?: boolean;
    onRename: (target: SyncTarget) => void;
    onPull: (target: SyncTarget) => void;
    onPush: (target: SyncTarget) => void;
    onRemove: (target: SyncTarget) => void;
};

export function SyncTargetRow({
    target,
    running,
    tr,
    disabled = false,
    onRename,
    onPull,
    onPush,
    onRemove,
}: SyncTargetRowProps) {
    const isLan = target.type === 'lan';
    const protocolLabel = isLan ? 'LAN' : 'TT-Sync';
    const lastSyncText = target.lastSyncMs
        ? formatTimestampValue(target.lastSyncMs, tr)
        : tr('Never');
    const secondaryLine = isLan
        ? target.lastKnownAddress || tr('Address: N/A (reconnect needed)')
        : target.baseUrl;
    const pullDisabled = disabled || (isLan && !target.lastKnownAddress);
    const pushDisabled = disabled || (isLan && (!target.lastKnownAddress || !running));
    const pullTitle = isLan && !target.lastKnownAddress
        ? tr('Address missing. Reconnect using Pair URI.')
        : tr(isLan ? 'Download (pull from this device)' : 'Download (pull from this server)');
    const pushTitle = (() => {
        if (isLan && !target.lastKnownAddress) {
            return tr('Address missing. Reconnect using Pair URI.');
        }
        if (isLan && !running) {
            return tr('Start LAN Sync server first (peer needs to download from you).');
        }
        return tr(isLan ? 'Upload (request device to pull from you)' : 'Upload (push to this server)');
    })();
    const removeTitle = tr(isLan ? 'Remove device' : 'Remove server');

    return (
        <div className={`tt-sync-target-row ${isLan ? 'tt-sync-target-lan' : 'tt-sync-target-tt'}`}>
            <div className="tt-sync-target-main">
                <button
                    type="button"
                    className="tt-sync-target-name"
                    title={tr('Click to rename')}
                    disabled={disabled}
                    onClick={() => onRename(target)}
                >
                    <b>{target.displayName}</b>
                    <i className="fa-solid fa-pen-to-square" aria-hidden="true"></i>
                </button>
                <div className="tt-sync-target-muted">{target.id}</div>
                <div className="tt-sync-target-muted tt-sync-target-address">
                    <span>{secondaryLine}</span>
                    <code>{protocolLabel}</code>
                </div>
                <div className="tt-sync-target-muted">{tr('Last sync')}: {lastSyncText}</div>
            </div>
            <div className="tt-sync-target-actions">
                <SyncButton
                    label={tr('Download')}
                    icon="fa-download"
                    iconOnly
                    title={pullTitle}
                    disabled={pullDisabled}
                    onClick={() => onPull(target)}
                />
                <SyncButton
                    label={tr('Upload')}
                    icon="fa-upload"
                    iconOnly
                    title={pushTitle}
                    disabled={pushDisabled}
                    onClick={() => onPush(target)}
                />
                <SyncButton
                    label={tr('Remove')}
                    icon="fa-trash-can"
                    iconOnly
                    title={removeTitle}
                    disabled={disabled}
                    onClick={() => onRemove(target)}
                />
            </div>
        </div>
    );
}
