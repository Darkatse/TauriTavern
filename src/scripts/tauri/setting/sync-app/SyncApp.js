import {
    SyncButton,
    SyncSection,
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
                return this.pairingInfo?.v2PairUri || '';
            },
            pairExpiryText() {
                return this.pairingInfo?.expiresAtMs
                    ? formatTimestampValue(this.pairingInfo.expiresAtMs, this.tr)
                    : this.tr('N/A');
            },
            qrImageSrc() {
                const svg = this.pairingInfo?.v2QrSvg || '';
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
            applySnapshot(snapshot) {
                this.status = snapshot.status;
                this.devices = snapshot.devices;
                this.servers = snapshot.servers;
                this.selectedAddress = snapshot.selectedAddress || '';
                this.datasetCatalog = snapshot.datasetCatalog;
                this.syncSelection = snapshot.syncSelection;
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
                    }
                });
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
                        await client.pullLanDevice(target.id, options);
                        return;
                    }

                    const mode = this.status?.syncMode ?? 'Incremental';
                    await client.pullTtSyncServer(target.id, mode, options);
                });
            },
            async pushTarget(target) {
                await this.withBusy(`push:${target.type}:${target.id}`, async () => {
                    const options = this.syncOperationOptions();
                    if (target.type === 'lan') {
                        await client.pushLanDevice(target.id, options);
                        actions.notifyLanPushRequested();
                        return;
                    }

                    const mode = this.status?.syncMode ?? 'Incremental';
                    await client.pushTtSyncServer(target.id, mode, options);
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
                    </div>
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

                <SyncSection :title="tr('Pair via LAN v2 QR')">
                    <div class="tt-sync-pair-grid">
                        <div class="tt-sync-qr-wrap">
                            <img v-if="qrImageSrc" :src="qrImageSrc" alt="LAN Sync v2 Pair QR" width="200" height="200" />
                            <span v-else>{{ tr('No QR') }}</span>
                        </div>
                        <div class="tt-sync-pair-fields">
                            <div class="tt-sync-muted">{{ tr('Expires') }}: <code>{{ pairExpiryText }}</code></div>
                            <textarea
                                class="text_pole tt-sync-textarea"
                                :value="pairUri"
                                rows="4"
                                readonly
                                :placeholder="tr('LAN Sync v2 Pair URI')"
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
