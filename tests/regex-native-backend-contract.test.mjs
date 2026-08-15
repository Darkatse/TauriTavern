import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { getRequiredTagLiteral } from '../src/scripts/extensions/regex/literal-gate.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('native regex batching shares the same runnable-script gate as sync regex execution', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src', 'scripts', 'extensions', 'regex', 'engine.js'), 'utf8');

    assert.match(source, /function canRunRegexScript\(regexScript\)\s*\{/);
    assert.match(source, /return\s+!!regexScript\s+&&\s+!regexScript\.disabled\s+&&\s+!!regexScript\.findRegex;/);
    assert.match(source, /function isRegexScriptActiveForParams\(script,[\s\S]*?if \(!canRunRegexScript\(script\)\) \{\s*return false;\s*\}/);
    assert.match(source, /function runRegexScript\(regexScript,[\s\S]*?if \(!canRunRegexScript\(regexScript\) \|\| !rawString\) \{/);
});

test('native regex DTO remains an execution payload, not SillyTavern extension state', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src-tauri', 'crates', 'tt-application', 'src', 'dto', 'native_regex_dto.rs'), 'utf8');

    assert.doesNotMatch(source, /\bpub\s+disabled\b/);
});

test('literal gate only accepts provable case-sensitive tag prefixes', () => {
    const tagPatterns = [
        [/<UpdateVariable>[\s\S]*?<\/UpdateVariable>/gm, '<UpdateVariable'],
        [/<StatusBlock[^>]*>[\s\S]*?<\/StatusBlock>/g, '<StatusBlock'],
        [/^.*?<\/customize_cot>/s, '</customize_cot'],
        [/<safe>.*?<\/safe>/gs, '<safe'],
        [/<宿命>[\s\S]*?<\/宿命>/gm, '<宿命'],
        [/<StatusPlaceHolderImpl\/>/g, '<StatusPlaceHolderImpl'],
        [/<Dice1\/>/g, '<Dice1'],
    ];

    for (const [regex, literal] of tagPatterns) {
        assert.equal(getRequiredTagLiteral(regex), literal);
    }

    assert.equal(getRequiredTagLiteral(/<safe>|plain/g), null);
    assert.equal(getRequiredTagLiteral(/<safe>.*?<\/safe>/gi), null);
    assert.equal(getRequiredTagLiteral(/<(safe)>/g), null);
    assert.equal(getRequiredTagLiteral(/<safe?>/g), null);
});
