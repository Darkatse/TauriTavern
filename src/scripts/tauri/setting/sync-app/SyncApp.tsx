import { StrictMode, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';

import {
    automationTargetValue,
    validateSyncMainBoundary,
    type SyncMainHandle,
    type SyncTarget,
    type SyncTranslate,
} from './SyncContract';
import { SyncButton, SyncSection, SyncSwitch, SyncTargetRow } from './SyncComponents';
import { createSyncController, type SyncController } from './SyncController';
import {
    automationStatusText,
    automationTargetOptions,
    formatAutomationInterval,
    formatShortTimestamp,
    scopeSummaryText,
} from './SyncText';

const AUTO_SYNC_INTERVAL_OPTIONS = [5, 15, 30, 60, 180, 360, 720, 1440];

type SyncMainViewProps = {
    controller: SyncController;
    canScanPairUri: boolean;
    tr: SyncTranslate;
};

function SyncMainView({ controller, canScanPairUri, tr }: SyncMainViewProps) {
    const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const { status, automationConfig: config } = state;

    const running = Boolean(status?.running);
    const pairingEnabled = Boolean(status?.pairingEnabled);
    const isBusy = state.loading || state.busy !== '';
    const targets: SyncTarget[] = [...state.devices, ...state.servers];
    const availableAddresses = status?.availableAddresses || [];
    const hasAddresses = availableAddresses.length > 0;

    const effectiveMode = status?.syncMode ?? 'Incremental';
    const modeLabel = effectiveMode === 'Mirror'
        ? tr(status?.syncModeOverridden ? 'Mirror Mode (session)' : 'Mirror Mode')
        : tr('Incremental Mode');
    const pairingText = pairingEnabled
        ? `${tr('Enabled')} · ${tr('Expires')} ${formatShortTimestamp(status?.pairingExpiresAtMs, tr)}`
        : tr('Disabled');

    const scopeText = scopeSummaryText(state.syncSelection, state.datasetCatalog, tr);

    const statusText = automationStatusText(state.automationStatus, tr);
    const targetOptions = automationTargetOptions(targets, config);
    const targetValue = automationTargetValue(config.target);
    const targetLabel = targetOptions.find(option => option.value === targetValue)?.label || tr('Choose target');
    const automationSummary = config.autoSyncEnabled
        ? [
            tr('On'),
            formatAutomationInterval(config.intervalMinutes, tr),
            tr(config.syncMode === 'Mirror' ? 'Mirror Mode' : 'Incremental Mode'),
            targetLabel,
        ].join(' · ')
        : `${tr('Off')} · ${statusText}`;
    const automationSaveDisabled = isBusy
        || !state.automationDraftDirty
        || !state.syncSelection
        || (config.autoSyncEnabled && !config.target);

    const pairUri = state.pairingInfo?.pairUri || '';
    const pairExpiryText = formatShortTimestamp(state.pairingInfo?.expiresAtMs, tr);
    const qrSvg = state.pairingInfo?.qrSvg || '';
    const qrImageSrc = qrSvg ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(qrSvg)}` : '';

    return (
        <div className="tt-sync-root">
            <header className="tt-sync-header">
                <div>
                    <b>{tr('Sync')}</b>
                </div>
                <SyncButton
                    label={modeLabel}
                    icon="fa-code-branch"
                    danger={status?.syncMode === 'Mirror'}
                    title={tr('Sync mode')}
                    disabled={isBusy}
                    onClick={() => void controller.changeSyncMode()}
                />
            </header>

            <section className="tt-sync-overview">
                <div className="tt-sync-status-line">
                    <span>{tr('Status')}</span>
                    <b className={`tt-sync-status-pill ${running ? 'running' : 'stopped'}`}>
                        {tr(running ? 'Running' : 'Stopped')}
                    </b>
                </div>
                <label className="tt-sync-address-row">
                    <span>{tr('Address')}</span>
                    <select
                        value={state.selectedAddress}
                        className="text_pole tt-sync-address-select"
                        disabled={!hasAddresses}
                        title={tr('Address')}
                        onChange={event => void controller.selectAddress(event.target.value)}
                    >
                        {!hasAddresses && <option value="">{tr('N/A')}</option>}
                        {availableAddresses.map(address => (
                            <option key={address} value={address}>{address}</option>
                        ))}
                    </select>
                </label>
                <div className="tt-sync-status-line">
                    <span>{tr('Pairing')}</span>
                    <b className={`tt-sync-status-pill ${pairingEnabled ? 'running' : 'stopped'}`}>
                        {pairingText}
                    </b>
                </div>
                <div className="tt-sync-actions">
                    {!running && (
                        <SyncButton
                            label={tr('Start')}
                            icon="fa-play"
                            disabled={isBusy}
                            onClick={() => void controller.startServer()}
                        />
                    )}
                    {running && (
                        <SyncButton
                            label={tr('Stop')}
                            icon="fa-stop"
                            disabled={isBusy}
                            onClick={() => void controller.stopServer()}
                        />
                    )}
                    <SyncSwitch
                        checked={config.lanServerAutoStart}
                        label={tr('Auto-start port')}
                        title={tr('Start sync port with app startup')}
                        disabled={isBusy}
                        onChange={enabled => void controller.setLanServerAutoStart(enabled)}
                    />
                </div>
            </section>

            <SyncSection title={tr('Sync preferences')}>
                <div className="tt-sync-preferences-card">
                    <div className="tt-sync-preference-row">
                        <div className="tt-sync-preference-copy">
                            <b>{tr('Sync content')}</b>
                            <span className="tt-sync-muted">{scopeText}</span>
                        </div>
                        <SyncButton
                            label={tr('Choose')}
                            icon="fa-list-check"
                            disabled={isBusy || !state.datasetCatalog}
                            onClick={() => void controller.editSyncScope()}
                        />
                    </div>
                    <div className="tt-sync-preference-row tt-sync-overwrite-row">
                        <div className="tt-sync-preference-copy">
                            <div className="tt-sync-preference-title">
                                <b>{tr('When files conflict')}</b>
                                <button
                                    type="button"
                                    className="tt-sync-help-button"
                                    title={tr('Learn more')}
                                    aria-label={tr('Learn more')}
                                    onClick={() => controller.showOverwritePolicyHelp()}
                                >
                                    <i className="fa-solid fa-circle-question" aria-hidden="true"></i>
                                </button>
                            </div>
                            <span id="tt-sync-overwrite-description" className="tt-sync-muted" aria-live="polite">
                                {tr(status?.overwritePolicy === 'prefer-newer'
                                    ? 'Keep the copy with the later modification time.'
                                    : "Keep the initiator's copy.")}
                            </span>
                        </div>
                        <div
                            className="tt-sync-overwrite-options"
                            role="radiogroup"
                            aria-label={tr('When files conflict')}
                            aria-describedby="tt-sync-overwrite-description"
                        >
                            {([
                                { value: 'exact', label: tr('Initiator wins (default)') },
                                { value: 'prefer-newer', label: tr('Newer copy wins') },
                            ] as const).map(option => (
                                <label key={option.value} className="tt-sync-overwrite-option">
                                    <input
                                        type="radio"
                                        name="tt-sync-overwrite-policy"
                                        value={option.value}
                                        checked={status?.overwritePolicy === option.value}
                                        disabled={isBusy || !status}
                                        onChange={() => void controller.setOverwritePolicy(option.value)}
                                    />
                                    <span>{option.label}</span>
                                </label>
                            ))}
                        </div>
                    </div>
                </div>
            </SyncSection>

            <section className="tt-sync-section tt-sync-automation-section">
                <details
                    className="tt-sync-automation-disclosure"
                    open={state.automationExpanded}
                    onToggle={event => controller.setAutomationExpanded(event.currentTarget.open)}
                >
                    <summary
                        onClick={(event) => {
                            const target = event.target;
                            if (target instanceof Element && target.closest('.tt-sync-automation-switch-wrap')) {
                                // The label and summary both own click defaults; handle the switch explicitly.
                                event.preventDefault();
                                if (!isBusy) {
                                    void controller.setAutoSyncEnabled(!config.autoSyncEnabled);
                                }
                            }
                        }}
                    >
                        <span className="tt-sync-automation-title">
                            <b>{tr('Auto sync')}</b>
                        </span>
                        <span className="tt-sync-automation-summary-meta">
                            <small>{automationSummary}</small>
                            <span className="tt-sync-automation-switch-wrap">
                                <SyncSwitch
                                    checked={config.autoSyncEnabled}
                                    title={tr('Auto upload while app is running')}
                                    disabled={isBusy}
                                    onChange={enabled => void controller.setAutoSyncEnabled(enabled)}
                                />
                            </span>
                            <i className="fa-solid fa-chevron-down tt-sync-automation-chevron" aria-hidden="true"></i>
                        </span>
                    </summary>
                    <div className="tt-sync-automation-body">
                        <div className="tt-sync-automation-grid">
                            <label className="tt-sync-field-row">
                                <span>{tr('Interval')}</span>
                                <select
                                    value={config.intervalMinutes}
                                    className="text_pole"
                                    disabled={isBusy}
                                    onChange={event => controller.setAutomationInterval(event.target.value)}
                                >
                                    {AUTO_SYNC_INTERVAL_OPTIONS.map(minutes => (
                                        <option key={minutes} value={minutes}>
                                            {formatAutomationInterval(minutes, tr)}
                                        </option>
                                    ))}
                                </select>
                            </label>
                            <label className="tt-sync-field-row">
                                <span>{tr('Sync mode')}</span>
                                <select
                                    value={config.syncMode}
                                    className="text_pole"
                                    disabled={isBusy}
                                    onChange={event => controller.setAutomationMode(event.target.value)}
                                >
                                    <option value="Incremental">{tr('Incremental Mode')}</option>
                                    <option value="Mirror">{tr('Mirror Mode')}</option>
                                </select>
                            </label>
                            <label className="tt-sync-field-row tt-sync-field-row-wide">
                                <span>{tr('Target')}</span>
                                <select
                                    value={targetValue}
                                    className="text_pole"
                                    disabled={isBusy}
                                    onChange={event => controller.setAutomationTarget(event.target.value)}
                                >
                                    <option value="">{tr('Choose target')}</option>
                                    {targetOptions.map(option => (
                                        <option
                                            key={option.value}
                                            value={option.value}
                                            disabled={option.disabled}
                                        >
                                            {option.label}
                                        </option>
                                    ))}
                                </select>
                            </label>
                        </div>
                        <div className="tt-sync-auto-warning">
                            <i className="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
                            <span>
                                {tr('Auto sync only uploads from this device. Do not use or edit data on the target device while it is syncing; Mirror mode may delete target files.')}
                            </span>
                        </div>
                        <div className="tt-sync-scope-row">
                            <div className="tt-sync-scope-current">
                                <b>{tr('Auto sync status')}</b>
                                <span className="tt-sync-muted">{statusText}</span>
                            </div>
                            <SyncButton
                                label={tr('Save')}
                                icon="fa-floppy-disk"
                                disabled={automationSaveDisabled}
                                onClick={() => void controller.saveAutomation()}
                            />
                        </div>
                    </div>
                </details>
            </section>

            <SyncSection title={tr('Pairing')}>
                {state.pairingInfo ? (
                    <div className="tt-sync-pair-grid">
                        <div className="tt-sync-qr-wrap">
                            {qrImageSrc
                                ? <img src={qrImageSrc} alt="LAN Sync Pair QR" width={200} height={200} />
                                : <span>{tr('No QR')}</span>}
                        </div>
                        <div className="tt-sync-pair-fields">
                            <div className="tt-sync-muted">{tr('Expires')}: <code>{pairExpiryText}</code></div>
                            <textarea
                                className="text_pole tt-sync-textarea"
                                value={pairUri}
                                rows={4}
                                readOnly
                                placeholder={tr('LAN Sync Pair URI')}
                            />
                            <div className="tt-sync-actions">
                                <SyncButton
                                    label={tr('Copy URI')}
                                    icon="fa-copy"
                                    disabled={isBusy || !pairUri}
                                    onClick={() => void controller.copyPairUri()}
                                />
                                <SyncButton
                                    label={tr('Regenerate')}
                                    icon="fa-arrows-rotate"
                                    iconOnly
                                    title={tr('Regenerate')}
                                    disabled={isBusy}
                                    onClick={() => void controller.enablePairing()}
                                />
                            </div>
                        </div>
                    </div>
                ) : (
                    <div className="tt-sync-pair-share">
                        {running ? (
                            <SyncButton
                                label={tr('Enable Pairing')}
                                icon="fa-qrcode"
                                disabled={isBusy}
                                onClick={() => void controller.enablePairing()}
                            />
                        ) : (
                            <span className="tt-sync-muted">
                                {tr('Start the server to share this device for pairing.')}
                            </span>
                        )}
                    </div>
                )}

                <div className="tt-sync-pair-connect">
                    <textarea
                        value={state.requestPairUri}
                        className="text_pole tt-sync-textarea"
                        rows={3}
                        placeholder={tr('Paste Pair URI here (pairs new or reconnects existing)')}
                        onChange={event => controller.setRequestPairUri(event.target.value)}
                    />
                    <div className="tt-sync-actions">
                        {canScanPairUri && (
                            <SyncButton
                                label={tr('Scan')}
                                icon="fa-camera"
                                disabled={isBusy}
                                onClick={() => void controller.scanPairing()}
                            />
                        )}
                        <SyncButton
                            label={tr('Connect')}
                            icon="fa-link"
                            disabled={isBusy}
                            onClick={() => void controller.connectPairing()}
                        />
                    </div>
                </div>
            </SyncSection>

            <SyncSection
                title={tr('Paired devices')}
                actions={(
                    <SyncButton
                        label={tr('Refresh')}
                        icon="fa-arrows-rotate"
                        iconOnly
                        title={tr('Refresh')}
                        disabled={isBusy}
                        onClick={() => void controller.refresh()}
                    />
                )}
            >
                {targets.length === 0
                    ? <div className="tt-sync-empty">{tr('No paired devices')}</div>
                    : (
                        <div className="tt-sync-target-list">
                            {targets.map(target => (
                                <SyncTargetRow
                                    key={`${target.type}:${target.id}`}
                                    target={target}
                                    running={running}
                                    tr={tr}
                                    disabled={isBusy}
                                    onRename={item => void controller.renameTarget(item)}
                                    onPull={item => void controller.pullTarget(item)}
                                    onPush={item => void controller.pushTarget(item)}
                                    onRemove={item => void controller.removeTarget(item)}
                                />
                            ))}
                        </div>
                    )}
            </SyncSection>
        </div>
    );
}

export function mountTauriTavernSyncApp(
    mount: unknown,
    options: unknown,
): SyncMainHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync mount element is required');
    }
    validateSyncMainBoundary(options);
    const { client, actions, canScanPairUri = false, tr } = options;

    const controller = createSyncController({ client, actions, tr });
    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <SyncMainView controller={controller} canScanPairUri={canScanPairUri} tr={tr} />
        </StrictMode>,
    );
    // The initial load is owned by the mount, not by a React effect, so
    // StrictMode's double render cannot start it twice.
    void controller.refresh();

    return {
        refresh: () => controller.refresh(),
        refreshAutomationStatus: () => controller.refreshAutomationStatus(),
        unmount: () => root.unmount(),
    };
}
