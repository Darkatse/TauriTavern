import { StrictMode, useState, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';

import {
    automationTargetValue,
    validateSyncMainBoundary,
    type SyncMainHandle,
    type SyncTranslate,
} from './SyncContract';
import { SyncButton, SyncDeviceCard, SyncOverview, SyncSwitch } from './SyncComponents';
import { createSyncController, type SyncController } from './SyncController';
import { deviceCardList, VISIBLE_DEVICE_CARD_LIMIT } from './SyncDevices';
import {
    automationStatusText,
    automationTargetOptions,
    formatAutomationInterval,
    scopeSummaryText,
} from './SyncText';

const AUTO_SYNC_INTERVAL_OPTIONS = [5, 15, 30, 60, 180, 360, 720, 1440];

type SyncMainViewProps = {
    controller: SyncController;
    canScanPairUri: boolean;
    tr: SyncTranslate;
};

/**
 * Layout follows task frequency: run state on top, actionable devices in the
 * golden zone, and everything rare (pairing links, preferences, automation)
 * folded away underneath.
 */
function SyncMainView({ controller, canScanPairUri, tr }: SyncMainViewProps) {
    const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const [showAllDevices, setShowAllDevices] = useState(false);
    const [pairingOpen, setPairingOpen] = useState(false);
    const { status } = state;
    const config = { ...state.automationConfig, ...state.automationDraft };

    const running = Boolean(status?.running);
    const isBusy = state.loading || state.busy !== '';

    const cards = deviceCardList(state);
    const visibleCards = showAllDevices ? cards : cards.slice(0, VISIBLE_DEVICE_CARD_LIMIT);

    const effectiveMode = status?.syncMode ?? 'Incremental';
    const modeLabel = effectiveMode === 'Mirror'
        ? tr(status?.syncModeOverridden ? 'Mirror Mode (session)' : 'Mirror Mode')
        : tr('Incremental Mode');
    const scopeText = scopeSummaryText(state.syncSelection, state.datasetCatalog, tr);
    const conflictLabel = tr(status?.overwritePolicy === 'prefer-newer'
        ? 'Newer copy wins'
        : 'Initiator wins (default)');

    const statusText = automationStatusText(state.automationStatus, tr);
    const targets = cards.flatMap(card => card.kind === 'paired' ? [card.target] : []);
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
        || !state.automationDraft
        || !state.syncSelection
        || (config.autoSyncEnabled && !config.target);
    const preferencesSummary = [
        scopeText,
        conflictLabel,
        config.autoSyncEnabled ? automationSummary : `${tr('Auto sync')}: ${tr('Off')}`,
    ].join(' · ');

    const pairUri = state.pairingInfo?.pairUri || '';
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

            <SyncOverview
                status={status}
                disabled={isBusy}
                tr={tr}
                onStart={() => void controller.startServer()}
                onStop={() => void controller.stopServer()}
            />

            <section className="tt-sync-section">
                <div className="tt-sync-section-header">
                    <b>{tr('Devices')}</b>
                    <div className="tt-sync-actions">
                        <SyncButton
                            label={tr('Connect manually')}
                            icon="fa-link"
                            disabled={isBusy}
                            onClick={() => void controller.connectLanAddress()}
                        />
                        <SyncButton
                            label={tr('Refresh')}
                            icon="fa-arrows-rotate"
                            iconOnly
                            title={tr('Refresh devices')}
                            disabled={isBusy || state.discovering}
                            onClick={() => void controller.refreshNearbyDevices()}
                        />
                    </div>
                </div>
                {state.discoveryError && (
                    <div className="tt-sync-auto-warning" role="status">
                        <i className="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
                        <div>
                            <div>{tr('Device discovery failed. Refresh to try again.')}</div>
                            <small>{tr(state.discoveryError)}</small>
                        </div>
                    </div>
                )}
                {cards.length === 0 && !state.discoveryError && (
                    <div className="tt-sync-empty" role="status">
                        {state.discovering ? tr('Searching for devices...') : (
                            <>
                                <span>{tr('No devices yet')}</span>
                                <span className="tt-sync-empty-hint">
                                    {tr('Open Sync on your other device and it will appear here automatically.')}
                                </span>
                                <button
                                    type="button"
                                    className="tt-sync-text-button"
                                    onClick={() => setPairingOpen(true)}
                                >
                                    {tr('Pair via link or QR code')}
                                </button>
                            </>
                        )}
                    </div>
                )}
                {visibleCards.map(card => (
                    <SyncDeviceCard
                        key={`${card.kind === 'paired' ? card.target.type : card.kind}:${card.id}`}
                        card={card}
                        running={running}
                        tr={tr}
                        disabled={isBusy}
                        onPair={device => void controller.pairNearbyDevice(device)}
                        onRename={target => void controller.renameTarget(target)}
                        onPull={target => void controller.pullTarget(target)}
                        onPush={target => void controller.pushTarget(target)}
                        onRemove={target => void controller.removeTarget(target)}
                    />
                ))}
                {cards.length > VISIBLE_DEVICE_CARD_LIMIT && (
                    <button
                        type="button"
                        className="tt-sync-devices-toggle"
                        onClick={() => setShowAllDevices(value => !value)}
                    >
                        <span>
                            {showAllDevices
                                ? tr('Show fewer devices')
                                : `${tr('Show all devices')} (${cards.length})`}
                        </span>
                        <i
                            className={`fa-solid fa-chevron-down${showAllDevices ? ' is-flipped' : ''}`}
                            aria-hidden="true"
                        >
                        </i>
                    </button>
                )}
            </section>

            <details
                className="tt-sync-section tt-sync-fold tt-sync-pairing-fold"
                open={pairingOpen}
                onToggle={event => setPairingOpen(event.currentTarget.open)}
            >
                <summary>
                    <b className="tt-sync-fold-title">{tr('QR code or pairing link')}</b>
                    <i className="fa-solid fa-chevron-down tt-sync-fold-chevron" aria-hidden="true"></i>
                </summary>
                <div className="tt-sync-fold-body">
                    {state.pairingInfo ? (
                        <div className="tt-sync-pair-grid">
                            <div className="tt-sync-qr-wrap">
                                {qrImageSrc
                                    ? <img src={qrImageSrc} alt="LAN Sync Pair QR" width={200} height={200} />
                                    : <span>{tr('No QR')}</span>}
                            </div>
                            <div className="tt-sync-pair-fields">
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
                                        label={tr('Refresh')}
                                        icon="fa-arrows-rotate"
                                        iconOnly
                                        title={tr('Refresh QR code')}
                                        disabled={isBusy}
                                        onClick={() => void controller.showPairingInfo()}
                                    />
                                </div>
                            </div>
                        </div>
                    ) : (
                        <div className="tt-sync-pair-share">
                            {running ? (
                                <SyncButton
                                    label={tr('Show QR code')}
                                    icon="fa-qrcode"
                                    disabled={isBusy}
                                    onClick={() => void controller.showPairingInfo()}
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
                            placeholder={tr('Paste Pair URI here')}
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
                </div>
            </details>

            <details
                className="tt-sync-section tt-sync-fold tt-sync-preferences-fold"
            >
                <summary>
                    <b className="tt-sync-fold-title">{tr('Sync preferences')}</b>
                    <small className="tt-sync-fold-summary">{preferencesSummary}</small>
                    <i className="fa-solid fa-chevron-down tt-sync-fold-chevron" aria-hidden="true"></i>
                </summary>
                <div className="tt-sync-fold-body">
                    <div className="tt-sync-preferences-card">
                        <div className="tt-sync-preference-row">
                            <div className="tt-sync-preference-copy">
                                <b>{tr('Device name')}</b>
                                <span className="tt-sync-muted">{status?.deviceName || 'TauriTavern'}</span>
                            </div>
                            <SyncButton
                                label={tr('Rename')}
                                title={tr('Rename this device')}
                                disabled={isBusy || !status}
                                onClick={() => void controller.renameLocalDevice()}
                            />
                        </div>
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
                        <div className="tt-sync-preference-row">
                            <div className="tt-sync-preference-copy">
                                <b>{tr('Auto-start port')}</b>
                                <span className="tt-sync-muted">{tr('Start sync port with app startup')}</span>
                            </div>
                            <SyncSwitch
                                checked={config.lanServerAutoStart}
                                title={tr('Start sync port with app startup')}
                                disabled={isBusy}
                                onChange={enabled => void controller.setLanServerAutoStart(enabled)}
                            />
                        </div>
                    </div>

                    <div className="tt-sync-automation-block">
                        <div className="tt-sync-automation-head">
                            <div className="tt-sync-preference-copy">
                                <b>{tr('Auto sync')}</b>
                                <span className="tt-sync-muted">{automationSummary}</span>
                            </div>
                            <SyncSwitch
                                checked={config.autoSyncEnabled}
                                title={tr('Auto upload while app is running')}
                                disabled={isBusy}
                                onChange={enabled => void controller.setAutoSyncEnabled(enabled)}
                            />
                        </div>
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
                        <div className="tt-sync-automation-foot">
                            <span className="tt-sync-muted">{statusText}</span>
                            <SyncButton
                                label={tr('Save')}
                                icon="fa-floppy-disk"
                                disabled={automationSaveDisabled}
                                onClick={() => void controller.saveAutomation()}
                            />
                        </div>
                    </div>
                </div>
            </details>
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
    void controller.refreshNearbyDevices();

    return {
        refresh: () => controller.refresh(),
        refreshAutomationStatus: () => controller.refreshAutomationStatus(),
        unmount: () => {
            controller.dispose();
            root.unmount();
        },
    };
}
