import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
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

test('TauriTavern Sync popup wrapper owns host-only capabilities', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');

    assert.match(source, /window\.__TAURI__\?\.core\?\.invoke/);
    assert.match(source, /barcodeScanner/);
    assert.match(source, /callGenericPopup/);
    assert.match(source, /sync\.bundle\.js/);
    assert.match(source, /mountTauriTavernSyncApp/);
    assert.match(source, /mountTauriTavernSyncScopeApp/);
    assert.match(source, /parseTtSyncPairUri/);
    assert.match(source, /parseLanSyncV2PairUri/);
    assert.match(source, /sync_v2_get_dataset_catalog/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('TauriTavern Sync listeners keep event contract while delegating progress UI', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');

    assert.match(source, /lan_sync:progress/);
    assert.match(source, /lan_sync:completed/);
    assert.match(source, /lan_sync:error/);
    assert.match(source, /tt_sync:progress/);
    assert.match(source, /tt_sync:completed/);
    assert.match(source, /tt_sync:error/);
    assert.match(source, /mountTauriTavernSyncProgressApp/);
    assert.match(source, /window\.location\.reload\(\)/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
});

test('TauriTavern Sync automation status events refresh status only', async () => {
    const constants = await readRepoFile('src/scripts/tauri/setting/setting-panel/constants.js');
    const listeners = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');
    const popup = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');
    const entry = await readRepoFile('src/scripts/tauri/setting/sync-app/index.js');

    assert.match(constants, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);

    const statusBlock = listeners.slice(
        listeners.indexOf("listen('sync_auto:status'"),
        listeners.indexOf("listen('sync_auto:toast'"),
    );
    assert.match(statusBlock, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);
    assert.doesNotMatch(statusBlock, /SYNC_AUTOMATION_CHANGED_EVENT/);

    const toastBlock = listeners.slice(listeners.indexOf("listen('sync_auto:toast'"));
    assert.match(toastBlock, /SYNC_AUTOMATION_STATUS_CHANGED_EVENT/);

    assert.match(popup, /const refreshAutomationStatus = \(\) => \{[\s\S]*appHandle\.refreshAutomationStatus\(\)/);
    assert.match(popup, /addEventListener\(SYNC_AUTOMATION_STATUS_CHANGED_EVENT,\s*refreshAutomationStatus\)/);
    assert.doesNotMatch(popup, /addEventListener\(SYNC_AUTOMATION_STATUS_CHANGED_EVENT,\s*refresh\)/);
    assert.match(entry, /refreshAutomationStatus:\s*\(\)\s*=>\s*instance\.refreshAutomationStatus\(\)/);
});

test('TauriTavern Sync automation success toasts include next run time', async () => {
    const listeners = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-listeners.js');
    const service = await readRepoFile('src-tauri/src/application/services/sync_automation_service.rs');
    const model = await readRepoFile('src-tauri/src/domain/models/sync_automation.rs');
    const zhCn = await readRepoFile('src/locales/zh-cn.json');
    const zhTw = await readRepoFile('src/locales/zh-tw.json');

    assert.match(model, /pub next_run_at_ms:\s*Option<u64>/);
    assert.match(service, /emit_toast_with_next_run/);
    assert.match(service, /Auto sync upload has started as scheduled\./);
    assert.match(service, /Auto sync upload has completed as scheduled\./);
    assert.match(listeners, /formatTimestamp/);
    assert.match(listeners, /payload\?\.next_run_at_ms/);
    assert.match(listeners, /Auto sync upload has started as scheduled\. Next sync time: \$\{nextRun\}/);
    assert.match(listeners, /Auto sync upload has completed as scheduled\. Next sync time: \$\{nextRun\}/);
    assert.match(zhCn, /自动同步上传已经按计划开始。/);
    assert.match(zhCn, /自动同步上传已经按计划完成。/);
    assert.match(zhCn, /自动同步上传已经按计划开始，下次同步时间是\$\{0\}/);
    assert.match(zhCn, /自动同步上传已经按计划完成，下次同步时间是\$\{0\}/);
    assert.match(zhTw, /自動同步上傳已按計畫開始。/);
    assert.match(zhTw, /自動同步上傳已按計畫完成。/);
    assert.match(zhTw, /自動同步上傳已按計畫開始，下次同步時間是\$\{0\}/);
    assert.match(zhTw, /自動同步上傳已按計畫完成，下次同步時間是\$\{0\}/);
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

test('Rspack exposes a dedicated TauriTavern Sync Vue entry', async () => {
    const source = await readRepoFile('rspack.config.js');

    assert.match(source, /sync:\s*['"]\.\/src\/scripts\/tauri\/setting\/sync-app\/index\.js['"]/);
    assert.match(source, /listJavaScriptFiles\(['"]src\/scripts\/tauri\/setting\/sync-app['"]\)/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
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
    assert.match(source, /export\s+function\s+parseLanSyncV2PairUri/);
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
