import test from 'node:test';
import assert from 'node:assert/strict';
import { access, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

async function repoFileExists(relativePath) {
    try {
        await access(path.join(REPO_ROOT, relativePath));
        return true;
    } catch {
        return false;
    }
}

async function listJsFiles(relativeDir) {
    const root = path.join(REPO_ROOT, relativeDir);
    const results = [];
    const stack = [root];

    while (stack.length > 0) {
        const current = stack.pop();
        const entries = await readdir(current, { withFileTypes: true });
        for (const entry of entries) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                stack.push(fullPath);
                continue;
            }
            if (entry.isFile() && entry.name.endsWith('.js')) {
                results.push(path.relative(REPO_ROOT, fullPath).replace(/\\/g, '/'));
            }
        }
    }

    return results.sort();
}

function installLocalStorage(initial = {}) {
    const store = new Map(Object.entries(initial));
    globalThis.localStorage = {
        getItem: (key) => store.has(key) ? store.get(key) : null,
        setItem: (key, value) => {
            store.set(key, String(value));
        },
        removeItem: (key) => {
            store.delete(key);
        },
    };
    return globalThis.localStorage;
}

async function importSyncState() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauri/setting/setting-panel/sync-state.js',
    )).href);
}

async function importSyncApp() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauri/setting/sync-app/SyncApp.js',
    )).href);
}

async function importSyncJobEvents() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauri/setting/setting-panel/sync-job-events.js',
    )).href);
}

async function withMutedWarnings(task) {
    const warn = console.warn;
    console.warn = () => {};
    try {
        return await task();
    } finally {
        console.warn = warn;
    }
}

test('TauriTavern Sync popup wrapper owns host-only capabilities', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');

    assert.match(source, /window\.__TAURI__\?\.core\?\.invoke/);
    assert.match(source, /barcodeScanner/);
    assert.match(source, /callGenericPopup/);
    assert.match(source, /sync\.bundle\.js/);
    assert.match(source, /mountTauriTavernSyncApp/);
    assert.match(source, /mountTauriTavernSyncScopeApp/);
    assert.match(source, /parseTtSyncPairUri/);
    assert.match(source, /parseLanSyncPairUri/);
    assert.match(source, /sync_get_dataset_catalog/);
    assert.doesNotMatch(source, /sync_v2_get_dataset_catalog|getSyncV2DatasetSelection|setSyncV2DatasetSelection|v2 client/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('TauriTavern Sync listeners keep event contract while delegating progress UI', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');

    assert.match(source, /sync:job/);
    assert.doesNotMatch(source, /lan_sync:progress/);
    assert.doesNotMatch(source, /lan_sync:completed/);
    assert.doesNotMatch(source, /lan_sync:error/);
    assert.doesNotMatch(source, /tt_sync:progress/);
    assert.doesNotMatch(source, /tt_sync:completed/);
    assert.doesNotMatch(source, /tt_sync:error/);
    assert.match(source, /result\.status === 'failed'[\s\S]*remote_server[\s\S]*TT_SYNC_SERVERS_CHANGED_EVENT[\s\S]*showSyncError/);
    assert.match(source, /syncFailureRequiresReload/);
    assert.match(source, /const shouldReload = syncFailureRequiresReload\(result\)[\s\S]*showSyncError[\s\S]*if \(shouldReload\)[\s\S]*window\.location\.reload\(\)/);
    assert.match(source, /resolveSyncJobEventAction/);
    assert.match(source, /mountTauriTavernSyncProgressApp/);
    assert.match(source, /window\.location\.reload\(\)/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('TauriTavern Sync job event routing handles origin and status payloads', async () => {
    const { resolveSyncJobEventAction, syncFailureRequiresReload } = await importSyncJobEvents();
    const progress = {
        direction: 'Push',
        phase: 'Uploading',
        files_done: 1,
        files_total: 2,
        bytes_done: 3,
        bytes_total: 4,
        current_path: 'characters/a.png',
    };

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'progress',
        job: {
            endpoint: { type: 'remote_server', server_device_id: 'server-1' },
            origin: { type: 'manual' },
        },
        progress,
    }), {
        type: 'progress',
        title: 'TT-Sync progress',
        payload: progress,
    });

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'progress',
        job: {
            endpoint: { type: 'lan_peer', device_id: 'peer-1' },
            origin: { type: 'scheduled' },
        },
        progress,
    }), { type: 'ignore' });

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'failed',
        job: {
            endpoint: { type: 'remote_server', server_device_id: 'server-1' },
            origin: { type: 'manual' },
        },
        result: { status: 'failed', message: 'boom' },
    }), { type: 'ignore' });

    const remoteRequestCompleted = {
        status: 'completed',
        job: {
            endpoint: { type: 'lan_peer', device_id: 'peer-1' },
            intent: 'pull_to_local',
            origin: { type: 'remote_request', peer_id: 'peer-1' },
        },
        result: {
            status: 'completed',
            summary: { files_total: 1, bytes_total: 2, files_deleted: 0 },
        },
    };
    assert.deepEqual(resolveSyncJobEventAction(remoteRequestCompleted), {
        type: 'report',
        report: {
            job: remoteRequestCompleted.job,
            result: remoteRequestCompleted.result,
        },
    });

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'completed',
        job: {
            endpoint: { type: 'remote_server', server_device_id: 'server-1' },
            origin: { type: 'scheduled' },
        },
        result: { status: 'completed', summary: { files_total: 1, bytes_total: 2, files_deleted: 0 } },
    }), { type: 'ignore' });

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'completed',
        job: {
            endpoint: { type: 'lan_peer', device_id: 'peer-1' },
            origin: 'remote_request',
        },
        result: { status: 'completed', summary: { files_total: 1, bytes_total: 2, files_deleted: 0 } },
    }), { type: 'ignore' });

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'remote_request_accepted',
        job: {
            endpoint: { type: 'lan_peer', device_id: 'peer-1' },
            origin: { type: 'remote_request', peer_id: 'peer-1' },
        },
        result: { status: 'remote_request_accepted' },
    }), { type: 'ignore' });

    assert.equal(syncFailureRequiresReload({
        status: 'failed',
        failure_kind: 'after_partial_local_mutation',
    }), true);
    assert.equal(syncFailureRequiresReload({
        status: 'failed',
        failure_kind: 'without_local_mutation',
        reconcile_error: 'cache refresh failed',
    }), true);
    assert.equal(syncFailureRequiresReload({
        status: 'failed',
        failure_kind: 'without_local_mutation',
    }), false);
});

test('TauriTavern Sync automation status events refresh status only while toasts refresh panel data', async () => {
    const constants = await readRepoFile('src/scripts/tauri/setting/setting-panel/constants.js');
    const listeners = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');
    const popup = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');
    const entry = await readRepoFile('src/scripts/tauri/setting/sync-app/index.js');

    assert.match(constants, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);
    assert.match(constants, /SYNC_AUTOMATION_CHANGED_EVENT/);

    const statusBlock = listeners.slice(
        listeners.indexOf("listen('sync_auto:status'"),
        listeners.indexOf("listen('sync_auto:toast'"),
    );
    assert.match(statusBlock, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);
    assert.doesNotMatch(statusBlock, /SYNC_AUTOMATION_CHANGED_EVENT/);

    const toastBlock = listeners.slice(listeners.indexOf("listen('sync_auto:toast'"));
    assert.match(toastBlock, /SYNC_AUTOMATION_CHANGED_EVENT/);
    assert.doesNotMatch(toastBlock, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);

    assert.match(popup, /const refreshAutomationStatus = \(\) => \{[\s\S]*appHandle\.refreshAutomationStatus\(\)/);
    assert.match(popup, /addEventListener\(SYNC_AUTOMATION_STATUS_CHANGED_EVENT,\s*refreshAutomationStatus\)/);
    assert.doesNotMatch(popup, /addEventListener\(SYNC_AUTOMATION_STATUS_CHANGED_EVENT,\s*refresh\)/);
    assert.match(entry, /refreshAutomationStatus:\s*\(\)\s*=>\s*instance\.refreshAutomationStatus\(\)/);
});

test('TauriTavern Sync automation success toasts include next run time', async () => {
    const listeners = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');
    const service = await readRepoFile('src-tauri/src/application/services/sync_automation_service.rs');
    const model = await readRepoFile('src-tauri/crates/tt-contracts/src/sync_automation.rs');
    const zhCn = await readRepoFile('src/locales/zh-cn.json');
    const zhTw = await readRepoFile('src/locales/zh-tw.json');

    assert.match(model, /pub next_run_at_ms:\s*Option<u64>/);
    assert.match(service, /emit_toast_with_next_run/);
    assert.match(service, /Auto sync upload has completed as scheduled\./);
    assert.match(service, /Auto sync upload request has been accepted as scheduled\./);
    assert.match(listeners, /formatTimestamp/);
    assert.match(listeners, /payload\?\.next_run_at_ms/);
    assert.match(listeners, /Auto sync upload has completed as scheduled\. Next sync time: \$\{nextRun\}/);
    assert.match(listeners, /Auto sync upload request has been accepted as scheduled\. Next sync time: \$\{nextRun\}/);
    assert.match(zhCn, /自动同步上传已经按计划完成。/);
    assert.match(zhCn, /自动同步上传请求已按计划被接受。/);
    assert.match(zhCn, /自动同步上传已经按计划完成，下次同步时间是\$\{0\}/);
    assert.match(zhCn, /自动同步上传请求已按计划被接受，下次同步时间是\$\{0\}/);
    assert.match(zhTw, /自動同步上傳已按計畫完成。/);
    assert.match(zhTw, /自動同步上傳請求已按計畫被接受。/);
    assert.match(zhTw, /自動同步上傳已按計畫完成，下次同步時間是\$\{0\}/);
    assert.match(zhTw, /自動同步上傳請求已按計畫被接受，下次同步時間是\$\{0\}/);
});

test('TauriTavern Sync automation draft survives background refreshes', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/sync-app/SyncApp.js');

    assert.match(source, /automationDraftDirty:\s*false/);
    assert.match(source, /setAutomationInterval\(value\)\s*\{[\s\S]*this\.automationDraftDirty\s*=\s*true/);
    assert.match(source, /this\.automationConfig\.target\s*=\s*parseAutomationTargetValue\(value\);[\s\S]*this\.automationDraftDirty\s*=\s*true/);
    assert.match(source, /if \(!this\.automationDraftDirty\) \{[\s\S]*this\.automationConfig\s*=\s*snapshot\.automationConfig/);
    assert.match(source, /async refreshAutomationStatus\(\)\s*\{[\s\S]*client\.getAutomationStatus\(\)/);
    assert.match(source, /this\.automationDraftDirty\s*=\s*false/);
    assert.match(source, /@change="setAutomationInterval\(\$event\.target\.value\)"/);
});

test('TauriTavern Sync automation status shows the newest success-like timestamp', async () => {
    const { createTauriTavernSyncApp } = await importSyncApp();
    const noopFunctions = new Proxy({}, { get: () => () => {} });
    const component = createTauriTavernSyncApp({
        client: noopFunctions,
        actions: noopFunctions,
        tr: (key) => key,
    });
    const context = {
        ...component.data(),
        tr: (key) => key,
        automationStatus: {
            running: false,
            nextRunAtMs: null,
            lastAttemptAtMs: null,
            lastSuccessAtMs: 2000,
            lastRequestAcceptedAtMs: 1000,
            lastErrorAtMs: null,
            lastError: '',
        },
    };

    assert.match(component.computed.automationStatusText.call(context), /^Last success:/);

    context.automationStatus.lastSuccessAtMs = 1000;
    context.automationStatus.lastRequestAcceptedAtMs = 2000;
    assert.match(component.computed.automationStatusText.call(context), /^Last request accepted:/);
});

test('Rspack exposes a dedicated TauriTavern Sync Vue entry', async () => {
    const source = await readRepoFile('rspack.config.js');

    assert.match(source, /sync:\s*['"]\.\/src\/scripts\/tauri\/setting\/sync-app\/index\.js['"]/);
    assert.match(source, /listJavaScriptFiles\(['"]src\/scripts\/tauri\/setting\/sync-app['"]\)/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
});

test('TauriTavern Sync bundled UI does not contain legacy LAN v1 affordances', async () => {
    if (!await repoFileExists('src/scripts/tauri/setting/dist/sync.bundle.js')) {
        return;
    }

    const source = await readRepoFile('src/scripts/tauri/setting/dist/sync.bundle.js');

    assert.doesNotMatch(source, /v2PairUri|v2QrSvg|protocolVersion|LAN v1|LAN v2/);
    assert.doesNotMatch(source, /Pair via LAN v2 QR|LAN Sync v2 Pair URI/);
});

test('TauriTavern Sync dataset selection migrates legacy localStorage key once', async () => {
    const { getSyncDatasetSelection } = await importSyncState();
    const catalog = {
        policyVersion: 1,
        supportedDatasetIds: ['characters', 'chats'],
        defaultDatasetIds: ['characters'],
    };
    const legacy = JSON.stringify({ policy_version: 1, dataset_ids: ['chats'] });
    const storage = installLocalStorage({
        'tauritavern:sync_v2_dataset_selection': legacy,
    });

    assert.deepEqual(await withMutedWarnings(() => getSyncDatasetSelection(catalog)), {
        policy_version: 1,
        dataset_ids: ['chats'],
    });
    assert.equal(storage.getItem('tauritavern:sync_v2_dataset_selection'), null);
    assert.equal(
        storage.getItem('tauritavern:sync_dataset_selection'),
        JSON.stringify({ policy_version: 1, dataset_ids: ['chats'] }),
    );

    assert.deepEqual(await withMutedWarnings(() => getSyncDatasetSelection(catalog)), {
        policy_version: 1,
        dataset_ids: ['chats'],
    });
});

test('TauriTavern Sync dataset selection rejects invalid current key before legacy fallback', async () => {
    const { getSyncDatasetSelection } = await importSyncState();
    const catalog = {
        policyVersion: 1,
        supportedDatasetIds: ['characters', 'chats'],
        defaultDatasetIds: ['characters'],
    };
    const storage = installLocalStorage({
        'tauritavern:sync_dataset_selection': '{bad',
        'tauritavern:sync_v2_dataset_selection': JSON.stringify({
            policy_version: 1,
            dataset_ids: ['chats'],
        }),
    });

    assert.throws(
        () => getSyncDatasetSelection(catalog),
        /Stored sync content selection is invalid/,
    );
    assert.equal(storage.getItem('tauritavern:sync_dataset_selection'), '{bad');
    assert.notEqual(storage.getItem('tauritavern:sync_v2_dataset_selection'), null);
});

test('TauriTavern Sync dataset selection rejects invalid legacy key', async () => {
    const { getSyncDatasetSelection } = await importSyncState();
    const catalog = {
        policyVersion: 1,
        supportedDatasetIds: ['characters', 'chats'],
        defaultDatasetIds: ['characters'],
    };
    const storage = installLocalStorage({
        'tauritavern:sync_v2_dataset_selection': '{bad',
    });

    assert.throws(
        () => getSyncDatasetSelection(catalog),
        /Stored sync content selection is invalid/,
    );
    assert.equal(storage.getItem('tauritavern:sync_v2_dataset_selection'), '{bad');
    assert.equal(storage.getItem('tauritavern:sync_dataset_selection'), null);
});

test('TauriTavern Sync Vue app stays presentation-only', async () => {
    const files = await listJsFiles('src/scripts/tauri/setting/sync-app');
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/index.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/SyncApp.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/SyncScopeApp.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/sync-app/SyncProgressApp.js'));

    const forbidden = [
        'popup.js',
        'tauri-bridge.js',
        'barcode-scanner-service.js',
        '__TAURI__',
        'lan_sync_',
        'tt_sync_',
    ];

    for (const file of files) {
        const source = await readRepoFile(file);
        for (const token of forbidden) {
            assert.doesNotMatch(source, new RegExp(token.replaceAll('.', '\\.')), `${file} contains ${token}`);
        }
    }

    const entry = await readRepoFile('src/scripts/tauri/setting/sync-app/index.js');
    assert.match(entry, /from\s+['"]vue\/dist\/vue\.esm-bundler\.js['"]/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSyncApp/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSyncScopeApp/);
    assert.match(entry, /export\s+function\s+mountTauriTavernSyncProgressApp/);
});

test('TauriTavern Sync pure state helpers keep pair URI validation explicit', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-state.js');

    assert.match(source, /export\s+function\s+parseTtSyncPairUri/);
    assert.match(source, /export\s+function\s+parseLanSyncPairUri/);
    assert.match(source, /export\s+function\s+getSyncDatasetSelection/);
    assert.match(source, /tauritavern:sync_dataset_selection/);
    assert.match(source, /tauritavern:sync_v2_dataset_selection/);
    assert.doesNotMatch(source, /getSyncV2DatasetSelection|setSyncV2DatasetSelection/);
    assert.match(source, /Pair URI must start with tauritavern:\/\//);
    assert.match(source, /Pair URI is not a TT-Sync pairing link/);
    assert.match(source, /Pair URI is not a LAN Sync pairing link/);
    assert.match(source, /Pair URI must be v=2/);
    assert.match(source, /LAN Sync Pair URI must be v=2/);
    assert.match(source, /Pair URI missing url/);
    assert.match(source, /Pair URI missing token/);
    assert.match(source, /Pair URI missing spki/);
    assert.doesNotMatch(source, /callGenericPopup/);
    assert.doesNotMatch(source, /window\.__TAURI__/);
});
