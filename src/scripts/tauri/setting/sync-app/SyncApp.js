import {
    SyncButton,
    SyncSection,
    SyncSwitch,
    SyncTargetRow,
} from './components.js';
import { formatTimestampValue } from './format.js';

const REQUIRED_CLIENT_METHODS = [
    'loadState',
    'setAdvertiseAddress',
    'startLanServer',
    'stopLanServer',
    'enableLanPairing',
    'getLanPairingInfo',
    'removeLanDevice',
    'pullLanDevice',
    'pushLanDevice',
    'removeTtSyncServer',
    'pullTtSyncServer',
    'pushTtSyncServer',
    'updateAutomationConfig',
    'getAutomationStatus',
];

const REQUIRED_ACTIONS = [
    'copyText',
    'scanPairUri',
    'changeSyncMode',
    'editSyncScope',
    'renameTarget',
    'connectPairUri',
    'notifyLanPushRequested',
    'reportError',
    'showSyncReportResult',
];

function requireMethods(source, names, label) {
    for (const name of names) {
        if (typeof source?.[name] !== 'function') {
            throw new Error(`TauriTavern Sync ${label} is unavailable: ${name}`);
        }
    }
}

function normalizeBusyName(name) {
    return String(name || '').trim();
}

const AUTO_SYNC_INTERVAL_OPTIONS = [5, 15, 30, 60, 180, 360, 720, 1440];

function formatAutomationInterval(minutes, tr) {
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

function automationTargetValue(target) {
    if (!target?.type || !target?.id) {
        return '';
    }
    return `${target.type}:${target.id}`;
}

function parseAutomationTargetValue(value) {
    const raw = String(value || '').trim();
    if (!raw) {
        return null;
    }

    const separator = raw.indexOf(':');
    if (separator <= 0) {
        throw new Error(`Invalid auto sync target: ${raw}`);
    }

    return {
        type: raw.slice(0, separator),
        id: raw.slice(separator + 1),
    };
}

export function createTauriTavernSyncApp(options) {
    const {
        client,
        actions,
        canScanPairUri = false,
        tr,
    } = options || {};

    if (typeof tr !== 'function') {
        throw new Error('TauriTavern Sync translator is required');
    }
    requireMethods(client, REQUIRED_CLIENT_METHODS, 'client method');
    requireMethods(actions, REQUIRED_ACTIONS, 'action');

    return {
        name: 'TauriTavernSyncApp',
        components: {
            SyncButton,
            SyncSection,
            SyncSwitch,
            SyncTargetRow,
        },
        data() {
            return {
                status: null,
                devices: [],
                servers: [],
                selectedAddress: '',
                pairingInfo: null,
                datasetCatalog: null,
                syncSelection: null,
                automationConfig: {
                    lanServerAutoStart: false,
                    autoSyncEnabled: false,
                    intervalMinutes: 30,
                    target: null,
                    selection: null,
                },
                automationStatus: {
                    running: false,
                    nextRunAtMs: null,
                    lastAttemptAtMs: null,
                    lastSuccessAtMs: null,
                    lastError: '',
                },
                automationExpanded: false,
                automationDraftDirty: false,
                automationIntervals: AUTO_SYNC_INTERVAL_OPTIONS,
                requestPairUri: '',
                loading: false,
                busy: '',
                canScanPairUri: Boolean(canScanPairUri),
            };
        },
        computed: {
            running() {
                return Boolean(this.status?.running);
            },
            availableAddresses() {
                return this.status?.availableAddresses || [];
            },
            hasAddresses() {
                return this.availableAddresses.length > 0;
            },
            isBusy() {
                return this.loading || Boolean(this.busy);
            },
            pairingEnabled() {
                return Boolean(this.status?.pairingEnabled);
            },
            statusText() {
                return this.tr(this.running ? 'Running' : 'Stopped');
            },
            statusClass() {
                return this.running ? 'running' : 'stopped';
            },
            modeLabel() {
                const effective = this.status?.syncMode ?? 'Incremental';
                const overridden = Boolean(this.status?.syncModeOverridden);

                if (effective === 'Mirror') {
                    return this.tr(overridden ? 'Mirror Mode (session)' : 'Mirror Mode');
                }

                return this.tr('Incremental Mode');
            },
            modeDanger() {
                return this.status?.syncMode === 'Mirror';
            },
            pairingText() {
                if (!this.pairingEnabled) {
                    return this.tr('Disabled');
                }

                return `${this.tr('Enabled')} (${this.tr('Expires')}: ${formatTimestampValue(this.status?.pairingExpiresAtMs, this.tr)})`;
            },
            pairUri() {
                return this.pairingInfo?.pairUri || '';
            },
            pairExpiryText() {
                return this.pairingInfo?.expiresAtMs
                    ? formatTimestampValue(this.pairingInfo.expiresAtMs, this.tr)
                    : this.tr('N/A');
            },
            qrImageSrc() {
                const svg = this.pairingInfo?.qrSvg || '';
                if (!svg) {
                    return '';
                }

                return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
            },
            selectedDatasetCount() {
                return this.syncSelection?.dataset_ids?.length || 0;
            },
            supportedDatasetCount() {
                return this.datasetCatalog?.supportedDatasetIds?.length || 0;
            },
            defaultDatasetSelected() {
                const current = [...(this.syncSelection?.dataset_ids || [])].sort();
                const defaults = [...(this.datasetCatalog?.defaultDatasetIds || [])].sort();
                return current.length > 0
                    && current.length === defaults.length
                    && current.every((id, index) => id === defaults[index]);
            },
            scopeTitle() {
                if (!this.selectedDatasetCount) {
                    return this.tr('N/A');
                }
                return this.defaultDatasetSelected
                    ? this.tr('Recommended default')
                    : `${this.selectedDatasetCount} ${this.tr('datasets selected')}`;
            },
            scopeSubtitle() {
                if (!this.selectedDatasetCount || !this.supportedDatasetCount) {
                    return this.tr('Sync content selection is unavailable');
                }
                return `${this.selectedDatasetCount} / ${this.supportedDatasetCount}`;
            },
            targets() {
                return [
                    ...this.devices,
                    ...this.servers,
                ];
            },
            automationTargetValue: {
                get() {
                    return automationTargetValue(this.automationConfig?.target);
                },
                set(value) {
                    if (!this.automationConfig) {
                        return;
                    }
                    this.automationConfig.target = parseAutomationTargetValue(value);
                    this.automationDraftDirty = true;
                },
            },
            automationTargets() {
                return this.targets.map((target) => {
                    const isLan = target.type === 'lan';
                    const canWrite = isLan || Boolean(target.permissions?.write);
                    const canMirror = isLan || Boolean(target.permissions?.mirror_delete);
                    const disabled = isLan
                        ? !target.lastKnownAddress
                        : (!canWrite || (this.modeDanger && !canMirror));
                    const protocol = isLan ? 'LAN' : 'TT-Sync';

                    return {
                        value: automationTargetValue(target),
                        label: `${protocol} · ${target.displayName}`,
                        disabled,
                    };
                });
            },
            automationTargetLabel() {
                const value = this.automationTargetValue;
                if (!value) {
                    return this.tr('Choose target');
                }

                return this.automationTargets.find((target) => target.value === value)?.label
                    || this.tr('Choose target');
            },
            automationSummaryText() {
                if (!this.automationConfig?.autoSyncEnabled) {
                    return `${this.tr('Off')} · ${this.automationStatusText}`;
                }

                return [
                    this.tr('On'),
                    formatAutomationInterval(this.automationConfig.intervalMinutes, this.tr),
                    this.automationTargetLabel,
                ].join(' · ');
            },
            automationStatusText() {
                const status = this.automationStatus;
                if (!status) {
                    return this.tr('N/A');
                }
                if (status.running) {
                    return this.tr('Uploading...');
                }
                if (status.nextRunAtMs) {
                    return `${this.tr('Next run')}: ${formatTimestampValue(status.nextRunAtMs, this.tr)}`;
                }
                if (status.lastSuccessAtMs) {
                    return `${this.tr('Last success')}: ${formatTimestampValue(status.lastSuccessAtMs, this.tr)}`;
                }
                if (status.lastError) {
                    return `${this.tr('Last error')}: ${status.lastError}`;
                }
                return this.tr('Idle');
            },
            automationSaveDisabled() {
                if (this.isBusy || !this.automationConfig || !this.syncSelection) {
                    return true;
                }
                return this.automationConfig.autoSyncEnabled && !this.automationConfig.target;
            },
        },
        async mounted() {
            await this.refresh();
        },
        methods: {
            tr(key) {
                return tr(key);
            },
            reportError(error) {
                void actions.reportError(error);
            },
            async withBusy(name, task) {
                const busyName = normalizeBusyName(name);
                this.busy = busyName;
                try {
                    return await task();
                } catch (error) {
                    this.reportError(error);
                    return undefined;
                } finally {
                    if (this.busy === busyName) {
                        this.busy = '';
                    }
                }
            },
            async withBusyStrict(name, task) {
                const busyName = normalizeBusyName(name);
                this.busy = busyName;
                try {
                    return await task();
                } finally {
                    if (this.busy === busyName) {
                        this.busy = '';
                    }
                }
            },
            async runSyncCommand(command) {
                const report = await command();
                await actions.showSyncReportResult(report);
                return report;
            },
            automationIntervalLabel(minutes) {
                return formatAutomationInterval(minutes, this.tr);
            },
            setAutomationInterval(value) {
                this.automationConfig.intervalMinutes = Number(value);
                this.automationDraftDirty = true;
            },
            applySnapshot(snapshot) {
                this.status = snapshot.status;
                this.devices = snapshot.devices;
                this.servers = snapshot.servers;
                this.selectedAddress = snapshot.selectedAddress || '';
                this.datasetCatalog = snapshot.datasetCatalog;
                this.syncSelection = snapshot.syncSelection;
                if (!this.automationDraftDirty) {
                    this.automationConfig = snapshot.automationConfig;
                }
                this.automationStatus = snapshot.automationStatus;
            },
            async refresh() {
                this.loading = true;
                try {
                    this.applySnapshot(await client.loadState());
                } catch (error) {
                    this.reportError(error);
                } finally {
                    this.loading = false;
                }
            },
            async refreshAutomationStatus() {
                try {
                    this.automationStatus = await client.getAutomationStatus();
                } catch (error) {
                    this.reportError(error);
                }
            },
            async persistAutomationConfig() {
                if (!this.automationConfig) {
                    throw new Error(this.tr('Auto sync settings are unavailable'));
                }

                this.automationConfig = await client.updateAutomationConfig(
                    this.automationConfig,
                    this.syncSelection,
                );
                this.automationDraftDirty = false;
                this.automationStatus = await client.getAutomationStatus();
            },
            async changeSyncMode() {
                await this.withBusy('mode', async () => {
                    if (!this.status) {
                        await this.refresh();
                    }
                    if (await actions.changeSyncMode(this.status)) {
                        await this.refresh();
                    }
                });
            },
            syncOperationOptions() {
                if (!this.syncSelection) {
                    throw new Error(this.tr('Sync content selection is unavailable'));
                }

                return {
                    selection: this.syncSelection,
                    require_bundle_zstd: true,
                };
            },
            async editSyncScope() {
                await this.withBusy('scope', async () => {
                    const next = await actions.editSyncScope({
                        catalog: this.datasetCatalog,
                        selection: this.syncSelection,
                    });
                    if (next) {
                        this.syncSelection = next;
                        if (this.automationConfig) {
                            this.automationConfig.selection = next;
                            await this.persistAutomationConfig();
                        }
                    }
                });
            },
            async saveAutomation() {
                await this.withBusy('automation', async () => {
                    await this.persistAutomationConfig();
                });
            },
            async setLanServerAutoStart(enabled) {
                if (!this.automationConfig) {
                    return;
                }

                const previous = this.automationConfig.lanServerAutoStart;
                this.automationConfig.lanServerAutoStart = enabled;
                try {
                    await this.withBusyStrict('automation-port', async () => {
                        await this.persistAutomationConfig();
                    });
                } catch (error) {
                    this.automationConfig.lanServerAutoStart = previous;
                    this.reportError(error);
                }
            },
            async setAutoSyncEnabled(enabled) {
                if (!this.automationConfig) {
                    return;
                }

                const previous = this.automationConfig.autoSyncEnabled;
                this.automationConfig.autoSyncEnabled = enabled;
                if (enabled) {
                    this.automationExpanded = true;
                }

                try {
                    await this.withBusyStrict('automation', async () => {
                        await this.persistAutomationConfig();
                    });
                } catch (error) {
                    this.automationConfig.autoSyncEnabled = previous;
                    if (enabled) {
                        this.automationExpanded = true;
                    }
                    this.reportError(error);
                }
            },
            async startServer() {
                await this.withBusy('start', async () => {
                    await client.startLanServer();
                    await this.refresh();
                });
            },
            async stopServer() {
                await this.withBusy('stop', async () => {
                    await client.stopLanServer();
                    this.pairingInfo = null;
                    await this.refresh();
                });
            },
            async enablePairing() {
                await this.withBusy('pairing', async () => {
                    this.pairingInfo = await client.enableLanPairing(this.selectedAddress || null);
                    await this.refresh();
                });
            },
            async handleAddressChange() {
                await this.withBusy('address', async () => {
                    client.setAdvertiseAddress(this.selectedAddress);
                    if (this.pairingEnabled && this.selectedAddress) {
                        this.pairingInfo = await client.getLanPairingInfo(this.selectedAddress);
                    }
                });
            },
            async copyPairUri() {
                await this.withBusy('copyPairUri', async () => {
                    const value = this.pairUri.trim();
                    if (!value) {
                        throw new Error(this.tr('Pair URI is empty'));
                    }
                    await actions.copyText(value);
                });
            },
            async scanPairing() {
                await this.withBusy('scan', async () => {
                    const pairUri = await actions.scanPairUri();
                    if (pairUri === null) {
                        return;
                    }
                    this.requestPairUri = pairUri;
                    await this.connectPairing();
                });
            },
            async connectPairing() {
                await this.withBusy('connect', async () => {
                    const value = this.requestPairUri.trim();
                    if (!value) {
                        throw new Error(this.tr('Pair URI is empty'));
                    }
                    if (!await actions.connectPairUri(value)) {
                        return;
                    }
                    this.requestPairUri = '';
                    await this.refresh();
                });
            },
            async renameTarget(target) {
                await this.withBusy(`rename:${target.type}:${target.id}`, async () => {
                    if (await actions.renameTarget({
                        type: target.type,
                        id: target.id,
                        fallbackName: target.name,
                    })) {
                        await this.refresh();
                    }
                });
            },
            async pullTarget(target) {
                await this.withBusy(`pull:${target.type}:${target.id}`, async () => {
                    const options = this.syncOperationOptions();
                    if (target.type === 'lan') {
                        await this.runSyncCommand(() => client.pullLanDevice(target.id, options));
                        return;
                    }

                    const mode = this.status?.syncMode ?? 'Incremental';
                    await this.runSyncCommand(() => client.pullTtSyncServer(
                        target.id,
                        mode,
                        options,
                    ));
                });
            },
            async pushTarget(target) {
                await this.withBusy(`push:${target.type}:${target.id}`, async () => {
                    const options = this.syncOperationOptions();
                    if (target.type === 'lan') {
                        await this.runSyncCommand(() => client.pushLanDevice(target.id, options));
                        actions.notifyLanPushRequested();
                        return;
                    }

                    const mode = this.status?.syncMode ?? 'Incremental';
                    await this.runSyncCommand(() => client.pushTtSyncServer(
                        target.id,
                        mode,
                        options,
                    ));
                });
            },
            async removeTarget(target) {
                await this.withBusy(`remove:${target.type}:${target.id}`, async () => {
                    if (target.type === 'lan') {
                        await client.removeLanDevice(target.id);
                    } else {
                        await client.removeTtSyncServer(target.id);
                    }
                    await this.refresh();
                });
            },
        },
        template: `
            <div class="tt-sync-root">
                <header class="tt-sync-header">
                    <div>
                        <b>{{ tr('Sync') }}</b>
                    </div>
                    <SyncButton
                        :label="modeLabel"
                        icon="fa-code-branch"
                        :danger="modeDanger"
                        :title="tr('Sync mode')"
                        :disabled="isBusy"
                        @click="changeSyncMode"
                    />
                </header>

                <section class="tt-sync-overview">
                    <div class="tt-sync-status-line">
                        <span>{{ tr('Status') }}</span>
                        <b class="tt-sync-status-pill" :class="statusClass">{{ statusText }}</b>
                    </div>
                    <label class="tt-sync-address-row">
                        <span>{{ tr('Address') }}</span>
                        <select
                            v-model="selectedAddress"
                            class="text_pole tt-sync-address-select"
                            :disabled="!hasAddresses"
                            :title="tr('Address')"
                            @change="handleAddressChange"
                        >
                            <option v-if="!hasAddresses" value="">{{ tr('N/A') }}</option>
                            <option v-for="address in availableAddresses" :key="address" :value="address">
                                {{ address }}
                            </option>
                        </select>
                    </label>
                    <div class="tt-sync-status-line">
                        <span>{{ tr('Pairing') }}</span>
                        <b class="tt-sync-status-pill" :class="pairingEnabled ? 'running' : 'stopped'">
                            {{ pairingText }}
                        </b>
                    </div>
                    <div class="tt-sync-actions">
                        <SyncButton
                            v-if="!running"
                            :label="tr('Start')"
                            icon="fa-play"
                            :disabled="isBusy"
                            @click="startServer"
                        />
                        <SyncButton
                            v-if="running"
                            :label="tr('Stop')"
                            icon="fa-stop"
                            :disabled="isBusy"
                            @click="stopServer"
                        />
                        <SyncButton
                            v-if="running"
                            :label="tr('Enable Pairing')"
                            icon="fa-qrcode"
                            :disabled="isBusy"
                            @click="enablePairing"
                        />
                        <SyncSwitch
                            :model-value="automationConfig.lanServerAutoStart"
                            :label="tr('Auto-start port')"
                            :title="tr('Start sync port with app startup')"
                            :disabled="isBusy || !automationConfig"
                            @update:model-value="setLanServerAutoStart"
                        />
                    </div>
                </section>

                <section class="tt-sync-section tt-sync-automation-section">
                    <details
                        class="tt-sync-automation-disclosure"
                        :open="automationExpanded"
                        @toggle="automationExpanded = $event.currentTarget.open"
                    >
                        <summary>
                            <span class="tt-sync-automation-title">
                                <b>{{ tr('Auto sync') }}</b>
                            </span>
                            <span class="tt-sync-automation-summary-meta">
                                <small>{{ automationSummaryText }}</small>
                                <span
                                    class="tt-sync-automation-switch-wrap"
                                    @click.stop
                                    @keydown.stop
                                >
                                    <SyncSwitch
                                        :model-value="automationConfig.autoSyncEnabled"
                                        :title="tr('Auto upload while app is running')"
                                        :disabled="isBusy || !automationConfig"
                                        @update:model-value="setAutoSyncEnabled"
                                    />
                                </span>
                                <i class="fa-solid fa-chevron-down tt-sync-automation-chevron" aria-hidden="true"></i>
                            </span>
                        </summary>
                        <div class="tt-sync-automation-body">
                            <div class="tt-sync-automation-grid">
                                <label class="tt-sync-field-row">
                                    <span>{{ tr('Interval') }}</span>
                                    <select
                                        :value="automationConfig.intervalMinutes"
                                        class="text_pole"
                                        :disabled="isBusy || !automationConfig"
                                        @change="setAutomationInterval($event.target.value)"
                                    >
                                        <option v-for="minutes in automationIntervals" :key="minutes" :value="minutes">
                                            {{ automationIntervalLabel(minutes) }}
                                        </option>
                                    </select>
                                </label>
                                <label class="tt-sync-field-row tt-sync-field-row-wide">
                                    <span>{{ tr('Target') }}</span>
                                    <select
                                        v-model="automationTargetValue"
                                        class="text_pole"
                                        :disabled="isBusy || !automationConfig"
                                    >
                                        <option value="">{{ tr('Choose target') }}</option>
                                        <option
                                            v-for="target in automationTargets"
                                            :key="target.value"
                                            :value="target.value"
                                            :disabled="target.disabled"
                                        >
                                            {{ target.label }}
                                        </option>
                                    </select>
                                </label>
                            </div>
                            <div class="tt-sync-auto-warning">
                                <i class="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
                                <span>{{ tr('Auto sync only uploads from this device. Do not use or edit data on the target device while it is syncing; Mirror mode may delete target files.') }}</span>
                            </div>
                            <div class="tt-sync-scope-row">
                                <div class="tt-sync-scope-current">
                                    <b>{{ tr('Auto sync status') }}</b>
                                    <span class="tt-sync-muted">{{ automationStatusText }}</span>
                                </div>
                                <SyncButton
                                    :label="tr('Save')"
                                    icon="fa-floppy-disk"
                                    :disabled="automationSaveDisabled"
                                    @click="saveAutomation"
                                />
                            </div>
                        </div>
                    </details>
                </section>

                <SyncSection :title="tr('Sync content')">
                    <div class="tt-sync-scope-row">
                        <div class="tt-sync-scope-current">
                            <b>{{ scopeTitle }}</b>
                            <span class="tt-sync-muted">{{ scopeSubtitle }}</span>
                        </div>
                        <SyncButton
                            :label="tr('Choose')"
                            icon="fa-list-check"
                            :disabled="isBusy || !datasetCatalog"
                            @click="editSyncScope"
                        />
                    </div>
                </SyncSection>

                <SyncSection :title="tr('Pair via LAN QR')">
                    <div class="tt-sync-pair-grid">
                        <div class="tt-sync-qr-wrap">
                            <img v-if="qrImageSrc" :src="qrImageSrc" alt="LAN Sync Pair QR" width="200" height="200" />
                            <span v-else>{{ tr('No QR') }}</span>
                        </div>
                        <div class="tt-sync-pair-fields">
                            <div class="tt-sync-muted">{{ tr('Expires') }}: <code>{{ pairExpiryText }}</code></div>
                            <textarea
                                class="text_pole tt-sync-textarea"
                                :value="pairUri"
                                rows="4"
                                readonly
                                :placeholder="tr('LAN Sync Pair URI')"
                            ></textarea>
                            <div class="tt-sync-actions">
                                <SyncButton
                                    :label="tr('Copy URI')"
                                    icon="fa-copy"
                                    :disabled="isBusy || !pairUri"
                                    @click="copyPairUri"
                                />
                            </div>
                        </div>
                    </div>
                </SyncSection>

                <SyncSection :title="tr('Connect device')">
                    <textarea
                        v-model="requestPairUri"
                        class="text_pole tt-sync-textarea"
                        rows="3"
                        :placeholder="tr('Paste Pair URI here (pairs new or reconnects existing)')"
                    ></textarea>
                    <div class="tt-sync-actions">
                        <SyncButton
                            v-if="canScanPairUri"
                            :label="tr('Scan')"
                            icon="fa-camera"
                            :disabled="isBusy"
                            @click="scanPairing"
                        />
                        <SyncButton
                            :label="tr('Connect')"
                            icon="fa-link"
                            :disabled="isBusy"
                            @click="connectPairing"
                        />
                    </div>
                </SyncSection>

                <SyncSection :title="tr('Paired devices')">
                    <template #actions>
                        <SyncButton
                            :label="tr('Refresh')"
                            icon="fa-arrows-rotate"
                            icon-only
                            :title="tr('Refresh')"
                            :disabled="isBusy"
                            @click="refresh"
                        />
                    </template>

                    <div v-if="targets.length === 0" class="tt-sync-empty">{{ tr('No paired devices') }}</div>
                    <div v-else class="tt-sync-target-list">
                        <SyncTargetRow
                            v-for="target in targets"
                            :key="target.type + ':' + target.id"
                            :target="target"
                            :running="running"
                            :tr="tr"
                            :disabled="isBusy"
                            @rename="renameTarget"
                            @pull="pullTarget"
                            @push="pushTarget"
                            @remove="removeTarget"
                        />
                    </div>
                </SyncSection>
            </div>
        `,
    };
}
