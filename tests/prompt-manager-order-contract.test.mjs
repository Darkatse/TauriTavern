import test from 'node:test';
import assert from 'node:assert/strict';

import {
    isPromptManagerImportDataValid,
    repairNullPromptManagerEntries,
    resolvePromptOrderFromDomIdentifiers,
} from '../src/scripts/prompt-manager-order-utils.js';

test('prompt order drag resolution preserves existing reference objects', () => {
    const main = { identifier: 'main', enabled: true };
    const chatHistory = { identifier: 'chatHistory', enabled: true };

    const resolved = resolvePromptOrderFromDomIdentifiers(
        [main, chatHistory],
        ['chatHistory', 'main'],
    );

    assert.deepEqual(resolved, [chatHistory, main]);
    assert.equal(resolved[0], chatHistory);
    assert.equal(resolved[1], main);
});

test('prompt order drag resolution fails before producing null entries', () => {
    assert.throws(
        () => resolvePromptOrderFromDomIdentifiers(
            [{ identifier: 'main', enabled: true }],
            ['missing'],
        ),
        /missing a reference/,
    );

    assert.throws(
        () => resolvePromptOrderFromDomIdentifiers(
            [{ identifier: 'main', enabled: true }, null],
            ['main'],
        ),
        /prompt_order\[1\]/,
    );
});

test('prompt manager imports reject null prompt and order entries', () => {
    assert.equal(isPromptManagerImportDataValid({
        data: {
            prompts: [{ identifier: 'customPrompt' }],
            prompt_order: [{ identifier: 'customPrompt', enabled: false }],
        },
    }), true);

    assert.equal(isPromptManagerImportDataValid({
        data: {
            prompts: [null],
            prompt_order: [{ identifier: 'customPrompt', enabled: false }],
        },
    }), false);

    assert.equal(isPromptManagerImportDataValid({
        data: {
            prompts: [{ identifier: 'customPrompt' }],
            prompt_order: [null],
        },
    }), false);
});

test('prompt manager null repair is scoped to prompt arrays', () => {
    const settings = {
        selected_proxy: null,
        prompts: [
            null,
            { identifier: 'main' },
        ],
        prompt_order: [
            null,
            {
                character_id: 100001,
                metadata: null,
                order: [
                    { identifier: 'main', enabled: true },
                    null,
                    { identifier: 'chatHistory', enabled: true },
                ],
            },
        ],
    };

    assert.equal(repairNullPromptManagerEntries(settings), 3);
    assert.deepEqual(settings, {
        selected_proxy: null,
        prompts: [
            { identifier: 'main' },
        ],
        prompt_order: [
            {
                character_id: 100001,
                metadata: null,
                order: [
                    { identifier: 'main', enabled: true },
                    { identifier: 'chatHistory', enabled: true },
                ],
            },
        ],
    });
});
