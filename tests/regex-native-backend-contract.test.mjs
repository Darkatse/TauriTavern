import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { getRequiredTagLiteral } from '../src/scripts/extensions/regex/literal-gate.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');



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
