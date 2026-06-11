import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('TauriTavern panel popup adapter keeps Popup semantics while opting into mobile fullscreen surface', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/panel-popup.js');

    assert.match(source, /import\s+\{\s*Popup\s+\}\s+from\s+['"]\.\.\/\.\.\/popup\.js['"]/);
    assert.match(source, /TAURITAVERN_PANEL_POPUP_CLASS\s*=\s*['"]tt-tauritavern-panel-popup['"]/);
    assert.match(source, /FULLSCREEN_WINDOW_SURFACE\s*=\s*['"]fullscreen-window['"]/);
    assert.match(source, /new\s+Popup\(content,\s*type,\s*inputValue,\s*options\)/);
    assert.match(source, /popup\.dlg\.classList\.add\(TAURITAVERN_PANEL_POPUP_CLASS\)/);
    assert.match(source, /popup\.dlg\.setAttribute\(MOBILE_SURFACE_ATTR,\s*FULLSCREEN_WINDOW_SURFACE\)/);
    assert.match(source, /createTauriTavernPanelPopup\(content,\s*type,\s*inputValue,\s*options\)\.show\(\)/);
});

test('major TauriTavern panels use the dedicated panel popup shell', async () => {
    const settingsSource = await readRepoFile('src/scripts/tauri/setting/setting-panel/settings-popup.js');
    const devLogsSource = await readRepoFile('src/scripts/tauri/setting/dev-logs.js');
    const syncSource = await readRepoFile('src/scripts/tauri/setting/setting-panel/sync-popup.js');

    assert.match(settingsSource, /from\s+['"]\.\.\/panel-popup\.js['"]/);
    assert.match(settingsSource, /await\s+callTauriTavernPanelPopup\(mount,\s*POPUP_TYPE\.CONFIRM/);
    assert.match(devLogsSource, /from\s+['"]\.\/panel-popup\.js['"]/);
    assert.match(devLogsSource, /await\s+callTauriTavernPanelPopup\(mount,\s*POPUP_TYPE\.TEXT/);
    assert.match(syncSource, /from\s+['"]\.\.\/panel-popup\.js['"]/);
    assert.match(syncSource, /await\s+callTauriTavernPanelPopup\(mount,\s*POPUP_TYPE\.TEXT/);
});

test('mobile geometry firewall owns the TauriTavern panel popup surface geometry', async () => {
    const source = await readRepoFile('src/tauri/main/compat/mobile/mobile-geometry-firewall.js');

    assert.match(
        source,
        /body\s+\[data-tt-mobile-surface="fullscreen-window"\]\[data-tt-mobile-surface\]\[data-tt-mobile-surface\]\s*\{[\s\S]*position:\s*fixed\s*!important[\s\S]*top:\s*max\(var\(--tt-inset-top\),\s*0px\)\s*!important[\s\S]*bottom:\s*max\(var\(--tt-viewport-bottom-inset,\s*var\(--tt-inset-bottom\)\),\s*0px\)\s*!important[\s\S]*width:\s*auto\s*!important[\s\S]*height:\s*auto\s*!important[\s\S]*min-width:\s*0\s*!important[\s\S]*margin:\s*0\s*!important/,
    );
    assert.match(
        source,
        /body\s+dialog\.popup\.tt-tauritavern-panel-popup\[data-tt-mobile-surface="fullscreen-window"\]\[data-tt-mobile-surface\]\[data-tt-mobile-surface\]\s*\{[\s\S]*--tt-panel-popup-wide-width:[\s\S]*--tt-panel-popup-wide-height:[\s\S]*width:\s*min\(980px,\s*var\(--tt-panel-popup-wide-width\)\)\s*!important[\s\S]*height:\s*min\(760px,\s*var\(--tt-panel-popup-wide-height\)\)\s*!important[\s\S]*margin:\s*auto\s*!important/,
    );
});

test('TauriTavern panel popup CSS owns only the compact internal layout', async () => {
    const source = await readRepoFile('src/css/popup.css');

    assert.match(source, /@media\s+screen\s+and\s+\(max-width:\s*1000px\)\s*\{/);
    assert.doesNotMatch(source, /--tt-panel-popup-safe-top/);
    assert.doesNotMatch(
        source,
        /\.popup\.tt-tauritavern-panel-popup\[data-tt-mobile-surface="fullscreen-window"\]\s*\{[\s\S]*position:\s*fixed/,
    );
    assert.match(
        source,
        /\.popup\.tt-tauritavern-panel-popup\[data-tt-mobile-surface="fullscreen-window"\]\s*\{[\s\S]*padding:\s*0[\s\S]*overflow:\s*hidden/,
    );
    assert.match(
        source,
        /\.popup\.tt-tauritavern-panel-popup\[data-tt-mobile-surface="fullscreen-window"\]\s+\.popup-content\s*\{[\s\S]*min-height:\s*0[\s\S]*overflow-y:\s*auto/,
    );
    assert.match(
        source,
        /\.popup\.tt-tauritavern-panel-popup\[data-tt-mobile-surface="fullscreen-window"\]\s+\.popup-controls\s*\{[\s\S]*flex-wrap:\s*wrap/,
    );
});
