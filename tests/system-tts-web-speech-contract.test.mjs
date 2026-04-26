import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('System TTS requires a complete Web Speech synthesis API', async () => {
    const source = await readFile(
        path.join(REPO_ROOT, 'src/scripts/extensions/tts/system.js'),
        'utf8',
    );

    assert.match(source, /Speech synthesis API is not supported in this WebView/);
    assert.match(source, /typeof synth\?\.speak !== 'function'/);
    assert.match(source, /typeof synth\?\.getVoices !== 'function'/);
    assert.match(source, /typeof synth\?\.cancel !== 'function'/);
    assert.match(source, /typeof Utterance !== 'function'/);
    assert.match(source, /error\.severity = 'warning'/);
    assert.doesNotMatch(source, /new SpeechSynthesisUtterance/);
});

test('TTS warning severity updates status without an error toast', async () => {
    const source = await readFile(
        path.join(REPO_ROOT, 'src/scripts/extensions/tts/index.js'),
        'utf8',
    );

    assert.match(source, /error\?\.severity === 'warning'/);
    assert.match(source, /setTtsStatus\(message, 'warning'\)/);
    assert.match(source, /console\.warn\(message, error\)/);
    assert.match(source, /function handleTtsProviderError\(error\)[\s\S]*?if \(isTtsWarning\(error\)\)[\s\S]*?return;[\s\S]*?toastr\.error\(String\(error\)\)/);
});
