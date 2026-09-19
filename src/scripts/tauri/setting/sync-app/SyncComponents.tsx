import { formatTimestampValue } from './format';
import type { SyncDeviceCard } from './SyncDevices';
import { targetDisplayName } from './SyncText';
import type {
    SyncNearbyDevice,
    SyncStatus,
    SyncTarget,
    SyncTranslate,
} from './SyncContract';

const PLATFORM_ICONS: Record<string, { icon: string; label: string }> = {
    windows: { icon: 'fa-brands fa-windows', label: 'Windows' },
    macos: { icon: 'fa-brands fa-apple', label: 'macOS' },
    ios: { icon: 'fa-brands fa-app-store-ios', label: 'iOS' },
    android: { icon: 'fa-brands fa-android', label: 'Android' },
    linux: { icon: 'fa-brands fa-linux', label: 'Linux' },
};

function SyncPlatformIcon({ platform = '', server = false, tr }: { platform?: string; server?: boolean; tr: SyncTranslate }) {
    const { icon, label } = PLATFORM_ICONS[platform] || {
        icon: `fa-solid ${server ? 'fa-server' : 'fa-display'}`,
        label: tr(server ? 'Server' : 'Device'),
    };
    return <i className={`${icon} tt-sync-platform-icon`} role="img" aria-label={label} title={label}></i>;
}

export function SyncOverview({ status, disabled, tr, onStart, onStop }: {
    status: SyncStatus | null;
    disabled: boolean;
    tr: SyncTranslate;
    onStart: () => void;
    onStop: () => void;
}) {
    const running = Boolean(status?.running);
    return (
        <section className="tt-sync-overview">
            <div className="tt-sync-status-line">
                <span>{tr('Status')}</span>
                <b className={`tt-sync-status-pill ${running ? 'running' : 'stopped'}`}>
                    {tr(running ? 'Running' : 'Stopped')}
                </b>
                {running && status?.address && (
                    <span className="tt-sync-muted tt-sync-status-address" title={status.address}>
                        {status.address}
                    </span>
                )}
                <SyncButton label={tr(running ? 'Stop' : 'Start')} icon={running ? 'fa-stop' : 'fa-play'}
                    disabled={disabled} onClick={running ? onStop : onStart} />
            </div>
            <details className="tt-sync-connection-details">
                <summary>{tr('Connection details')}</summary>
                <div className="tt-sync-connection-addresses">
                    {status?.availableAddresses.length ? status.availableAddresses.join(' · ') : tr('N/A')}
                </div>
            </details>
        </section>
    );
}

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

type SyncSwitchProps = {
    checked: boolean;
    disabled?: boolean;
    title: string;
    onChange: (checked: boolean) => void;
};

export function SyncSwitch({
    checked,
    disabled = false,
    title,
    onChange,
}: SyncSwitchProps) {
    return (
        <label className={`tt-sync-switch${disabled ? ' is-disabled' : ''}`} title={title}>
            <input
                type="checkbox"
                checked={checked}
                disabled={disabled}
                aria-label={title}
                onChange={event => onChange(event.target.checked)}
            />
            <span className="tt-sync-switch-track" aria-hidden="true"></span>
        </label>
    );
}

type SyncDeviceCardProps = {
    card: SyncDeviceCard;
    running: boolean;
    tr: SyncTranslate;
    disabled?: boolean;
    onPair: (device: SyncNearbyDevice) => void;
    onRename: (target: SyncTarget) => void;
    onPull: (target: SyncTarget) => void;
    onPush: (target: SyncTarget) => void;
    onRemove: (target: SyncTarget) => void;
};

export function SyncDeviceCard({
    card,
    running,
    tr,
    disabled = false,
    onPair,
    onRename,
    onPull,
    onPush,
    onRemove,
}: SyncDeviceCardProps) {
    if (card.kind === 'nearby') {
        return (
            <div className="tt-sync-device-card is-lan">
                <div className="tt-sync-device-info">
                    <div className="tt-sync-device-title">
                        <SyncPlatformIcon platform={card.device.platform} tr={tr} />
                        <b>{card.device.name}</b>
                        <span className="tt-sync-device-pill is-new">{tr('New device')}</span>
                    </div>
                    {card.address && <div className="tt-sync-device-meta">{card.address}</div>}
                </div>
                <div className="tt-sync-device-actions">
                    <SyncButton
                        label={tr('Pair')}
                        icon="fa-link"
                        title={`${tr('Pair')} ${card.device.name}`}
                        disabled={disabled}
                        onClick={() => onPair(card.device)}
                    />
                </div>
            </div>
        );
    }

    const target = card.target;
    const isLan = target.type === 'lan';
    const lastSyncText = target.lastSyncMs
        ? formatTimestampValue(target.lastSyncMs, tr)
        : tr('Never');
    const secondaryLine = isLan
        ? target.lastKnownAddress || tr('N/A')
        : target.baseUrl;
    const pushDisabled = disabled || (isLan && !running);
    const pullTitle = tr(isLan ? 'Download (pull from this device)' : 'Download (pull from this server)');
    const pushTitle = (() => {
        if (isLan && !running) {
            return tr('Start LAN Sync server first (peer needs to download from you).');
        }
        return tr(isLan ? 'Upload (request device to pull from you)' : 'Upload (push to this server)');
    })();

    return (
        <div className={`tt-sync-device-card ${isLan ? 'is-lan' : 'is-tt'}`}>
            <div className="tt-sync-device-info">
                <div className="tt-sync-device-title">
                    <SyncPlatformIcon platform={isLan ? target.platform : ''} server={!isLan} tr={tr} />
                    <button
                        type="button"
                        className="tt-sync-device-name"
                        title={tr('Click to rename')}
                        disabled={disabled}
                        onClick={() => onRename(target)}
                    >
                        <b>{targetDisplayName(target)}</b>
                        <i className="fa-solid fa-pen-to-square" aria-hidden="true"></i>
                    </button>
                    {card.online && <span className="tt-sync-device-pill is-online">{tr('Online')}</span>}
                </div>
                <div className="tt-sync-device-meta">
                    <code>{isLan ? 'LAN' : 'TT-Sync'}</code>
                    <span>{secondaryLine}</span>
                    <span>{tr('Last sync')}: {lastSyncText}</span>
                </div>
            </div>
            <div className="tt-sync-device-actions">
                <SyncButton
                    label={tr('Download')}
                    icon="fa-download"
                    title={pullTitle}
                    disabled={disabled}
                    onClick={() => onPull(target)}
                />
                <SyncButton
                    label={tr('Upload')}
                    icon="fa-upload"
                    title={pushTitle}
                    disabled={pushDisabled}
                    onClick={() => onPush(target)}
                />
                <SyncButton
                    label={tr('Remove')}
                    icon="fa-trash-can"
                    iconOnly
                    title={tr(isLan ? 'Remove device' : 'Remove server')}
                    disabled={disabled}
                    onClick={() => onRemove(target)}
                />
            </div>
        </div>
    );
}
