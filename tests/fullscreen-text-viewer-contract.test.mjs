import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('fullscreen text viewer owns full-window popup sizing without mobile surface opt-in', async () => {
    const source = await readFile(
        path.join(REPO_ROOT, 'src/scripts/tauri/setting/text-viewer-popup.js'),
        'utf8',
    );
    const popupCss = await readFile(path.join(REPO_ROOT, 'src/css/popup.css'), 'utf8');

    assert.match(source, /textarea\.readOnly\s*=\s*true/);
    assert.match(source, /textarea\.inputMode\s*=\s*'none'/);
    assert.doesNotMatch(source, /textarea\.setAttribute\('autofocus'/);
    assert.match(source, /popup\.okButton\.removeAttribute\('autofocus'\)/);
    assert.match(source, /popup\.closeButton\.classList\.add\('result-control'\)/);
    assert.match(source, /popup\.closeButton\.setAttribute\('autofocus',\s*''\)/);
    assert.match(source, /tt-fullscreen-text-viewer-popup/);
    assert.doesNotMatch(source, /\bapplySurface\b/);
    assert.doesNotMatch(source, /\bSURFACE\.FullscreenWindow\b/);

    assert.match(
        popupCss,
        /\.popup\.tt-fullscreen-text-viewer-popup\s*\{[\s\S]*height:\s*var\(--tt-base-viewport-height,\s*100dvh\)/,
    );
    assert.match(
        popupCss,
        /\.popup\.tt-fullscreen-text-viewer-popup\s*\{[\s\S]*min-height:\s*0/,
    );
    assert.match(
        popupCss,
        /\.popup\.tt-fullscreen-text-viewer-popup\s+\.popup-content\s*\{[\s\S]*height:\s*100%/,
    );
});
