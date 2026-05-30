import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('native regex batching shares the same runnable-script gate as sync regex execution', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src', 'scripts', 'extensions', 'regex', 'engine.js'), 'utf8');

    assert.match(source, /function canRunRegexScript\(regexScript\)\s*\{/);
    assert.match(source, /return\s+!!regexScript\s+&&\s+!regexScript\.disabled\s+&&\s+!!regexScript\.findRegex;/);
    assert.match(source, /function isRegexScriptActiveForParams\(script,[\s\S]*?if \(!canRunRegexScript\(script\)\) \{\s*return false;\s*\}/);
    assert.match(source, /function runRegexScript\(regexScript,[\s\S]*?if \(!canRunRegexScript\(regexScript\) \|\| !rawString\) \{/);
});

test('native regex DTO remains an execution payload, not SillyTavern extension state', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src-tauri', 'src', 'application', 'dto', 'native_regex_dto.rs'), 'utf8');

    assert.doesNotMatch(source, /\bpub\s+disabled\b/);
});
