import { useLayoutEffect, useRef, useState } from 'react';

import type {
    SettingsActions,
    SettingsCapabilities,
    SettingsDataRootState,
    SettingsDraft,
    SettingsTranslate,
} from './SettingsContract';
import { ActionButton, SettingRow, SettingsSection, ToggleSwitch } from './SettingsComponents';
import { dataRootStatus, dataRootSummary, requestProxySummary } from './SettingsText';

type RequestProxyDraft = SettingsDraft['requestProxy'];

type SettingsSystemSectionProps = {
    capabilities: SettingsCapabilities;
    proxy: RequestProxyDraft;
    initialDataRoot: SettingsDataRootState | null;
    chooseDataRoot: SettingsActions['chooseDataRoot'];
    tr: SettingsTranslate;
    onPatchProxy: (patch: Partial<RequestProxyDraft>) => void;
};

/**
 * The "System" section: Data Directory projection and the Request Proxy
 * disclosure. The data root is committed immediately by the shell action and
 * is not part of the popup draft, so its projection state lives here.
 */
export function SettingsSystemSection({
    capabilities,
    proxy,
    initialDataRoot,
    chooseDataRoot,
    tr,
    onPatchProxy,
}: SettingsSystemSectionProps) {
    const [dataRootOpen, setDataRootOpen] = useState(false);
    const [proxyOpen, setProxyOpen] = useState(() => proxy.enabled);
    const [dataRoot, setDataRoot] = useState(initialDataRoot);
    const [dataRootBusy, setDataRootBusy] = useState(false);
    const [proxyFocusTick, setProxyFocusTick] = useState(0);
    const requestProxyUrlRef = useRef<HTMLInputElement>(null);

    // One-shot focus intent: enabling the proxy expands the disclosure and
    // focuses the URL field in the same commit, without timeouts. The tick
    // counter makes repeated enable toggles re-fire the effect.
    useLayoutEffect(() => {
        if (proxyFocusTick === 0) {
            return;
        }
        requestProxyUrlRef.current?.focus();
    }, [proxyFocusTick]);

    const systemVisible = capabilities.supportsDataRootSelection
        || capabilities.requestProxyAllowed
        || proxy.enabled;
    if (!systemVisible) {
        return null;
    }
    const proxyFieldsDisabled = !proxy.enabled || !capabilities.requestProxyAllowed;

    function setRequestProxyEnabled(enabled: boolean): void {
        onPatchProxy({ enabled });
        if (!enabled) {
            return;
        }
        setProxyOpen(true);
        setProxyFocusTick(tick => tick + 1);
    }

    async function chooseDataRootPath(): Promise<void> {
        setDataRootBusy(true);
        try {
            const selected = await chooseDataRoot();
            if (!selected) {
                return;
            }
            setDataRoot(prev => prev && {
                ...prev,
                configuredDataRoot: selected,
                migrationPending: true,
                migrationError: '',
            });
        } finally {
            setDataRootBusy(false);
        }
    }

    return (
        <SettingsSection title={tr('System')} icon="fa-sliders">
            {capabilities.supportsDataRootSelection && dataRoot && (
                <details
                    className="tt-settings-disclosure"
                    open={dataRootOpen}
                    onToggle={event => setDataRootOpen(event.currentTarget.open)}
                >
                    <summary>
                        <span>{tr('Data Directory')}</span>
                        <span className="tt-settings-summary-meta">
                            <small>{dataRootSummary(dataRoot, tr)}</small>
                            <i className="fa-solid fa-chevron-down" aria-hidden="true"></i>
                        </span>
                    </summary>
                    <div className="tt-settings-disclosure-body">
                        <SettingRow label={tr('Data Directory')}>
                            <div className="tt-settings-path-row">
                                <input
                                    className="text_pole"
                                    type="text"
                                    readOnly
                                    value={dataRoot.currentDataRoot}
                                    aria-label={tr('Data Directory')}
                                />
                                <ActionButton
                                    label={tr('Choose...')}
                                    icon="fa-folder-open"
                                    disabled={dataRootBusy}
                                    onClick={() => void chooseDataRootPath()}
                                />
                            </div>
                        </SettingRow>
                        {dataRootStatus(dataRoot, tr) && (
                            <small className="tt-settings-status">{dataRootStatus(dataRoot, tr)}</small>
                        )}
                        <small className="tt-settings-section-note">{tr('Data Directory hint')}</small>
                    </div>
                </details>
            )}

            {(capabilities.requestProxyAllowed || proxy.enabled) && (
                <details
                    className="tt-settings-disclosure"
                    open={proxyOpen}
                    onToggle={event => setProxyOpen(event.currentTarget.open)}
                >
                    <summary>
                        <span>{tr('Request Proxy')}</span>
                        <span className="tt-settings-summary-meta">
                            <small>{requestProxySummary(proxy, tr)}</small>
                            <i className="fa-solid fa-chevron-down" aria-hidden="true"></i>
                        </span>
                    </summary>
                    <div className="tt-settings-disclosure-body">
                        <SettingRow label={tr('Enable Request Proxy')}>
                            <ToggleSwitch
                                checked={proxy.enabled}
                                disabled={!capabilities.requestProxyAllowed && !proxy.enabled}
                                ariaLabel={tr('Enable Request Proxy')}
                                onChange={setRequestProxyEnabled}
                            />
                        </SettingRow>
                        <SettingRow label={tr('Request Proxy URL')}>
                            <input
                                ref={requestProxyUrlRef}
                                className="text_pole tt-settings-input"
                                type="text"
                                value={proxy.url}
                                disabled={proxyFieldsDisabled}
                                placeholder="http://127.0.0.1:7890"
                                aria-label={tr('Request Proxy URL')}
                                onChange={event => onPatchProxy({ url: event.target.value })}
                            />
                        </SettingRow>
                        <div className="tt-settings-stack">
                            <span>{tr('Bypass (one per line)')}</span>
                            <textarea
                                rows={6}
                                value={proxy.bypass}
                                disabled={proxyFieldsDisabled}
                                placeholder={'localhost\n127.0.0.1\n10.0.0.0/8'}
                                aria-label={tr('Bypass (one per line)')}
                                onChange={event => onPatchProxy({ bypass: event.target.value })}
                            ></textarea>
                            <small className="tt-settings-section-note">
                                {tr('Matching hosts will connect directly (no proxy).')}
                            </small>
                        </div>
                        <small className="tt-settings-section-note">{tr('Applies to all backend requests.')}</small>
                    </div>
                </details>
            )}
        </SettingsSection>
    );
}
