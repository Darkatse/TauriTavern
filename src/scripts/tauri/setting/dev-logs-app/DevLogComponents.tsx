import type { LiveLogEntry } from './DevLogsContract';
import { formatTime, levelClass, normalizeLevel } from './log-utils';

export function DevLogButton({ label, icon = '', disabled = false, title = '', iconOnly = false, onClick }: {
    label: string;
    icon?: string;
    disabled?: boolean;
    title?: string;
    iconOnly?: boolean;
    onClick: () => void;
}) {
    return (
        <button
            type="button"
            className="menu_button menu_button_icon tt-dev-log-button"
            title={title || label}
            aria-label={title || label}
            disabled={disabled}
            onClick={onClick}
        >
            {icon !== '' && <i className={`fa-solid ${icon}`} aria-hidden="true" />}
            {!iconOnly && <span>{label}</span>}
        </button>
    );
}

export function DevLogToggle({ checked, label, onChange }: {
    checked: boolean;
    label: string;
    onChange: (checked: boolean) => void;
}) {
    return (
        <label className="tt-dev-log-toggle">
            <input
                type="checkbox"
                checked={checked}
                onChange={event => onChange(event.target.checked)}
            />
            <span>{label}</span>
        </label>
    );
}

export function LogRow({ entry }: { entry: LiveLogEntry }) {
    return (
        <div className={`tt-dev-log-row ${levelClass(entry.level)}`}>
            <div className="tt-dev-log-prefix">
                <span className="tt-dev-log-time">{formatTime(entry.timestampMs)}</span>
                <span className="tt-dev-log-badge">{normalizeLevel(entry.level)}</span>
                {entry.target ? <span className="tt-dev-log-target">{entry.target}</span> : null}
            </div>
            <span className="tt-dev-log-message">{entry.message}</span>
        </div>
    );
}

export function TextPreviewSection({ title, text = '', placeholder = '', rows = 10, viewerTitle = '', wrap = 'soft', onExpand }: {
    title: string;
    text?: string;
    placeholder?: string;
    rows?: number;
    viewerTitle?: string;
    wrap?: 'soft' | 'off';
    onExpand: () => void;
}) {
    return (
        <section className="tt-dev-log-text-section">
            <div className="tt-dev-log-text-header">
                <span>{title}</span>
                <DevLogButton
                    label={title}
                    icon="fa-expand"
                    iconOnly
                    title={viewerTitle || title}
                    onClick={onExpand}
                />
            </div>
            <textarea
                className="text_pole tt-dev-log-textarea"
                rows={rows}
                readOnly
                spellCheck={false}
                placeholder={placeholder}
                wrap={wrap}
                value={text}
                aria-label={title}
            />
        </section>
    );
}
