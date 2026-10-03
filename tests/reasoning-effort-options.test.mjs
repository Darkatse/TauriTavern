import assert from 'node:assert/strict';
import test from 'node:test';
import { getEffectiveReasoningEffort, getReasoningEffortOptions } from '../src/scripts/tauri/generation-params/reasoning-effort-options.js';

test('A stored effort the connection does not offer is used as auto and kept as stored', () => {
    for (const settings of [
        // New preset (saved under Custom) on a native source.
        { chat_completion_source: 'deepseek', reasoning_effort: 'minimal' },
        // An OpenAI word on a Gemini format.
        { chat_completion_source: 'custom', custom_api_format: 'gemini_generate_content', reasoning_effort: 'none' },
    ]) {
        const stored = structuredClone(settings);
        assert.equal(getEffectiveReasoningEffort(settings), 'auto', JSON.stringify(settings));
        assert.deepEqual(settings, stored);
        const accepted = getReasoningEffortOptions(settings).find(value => value !== 'auto');
        assert.equal(getEffectiveReasoningEffort({ ...settings, reasoning_effort: accepted }), accepted);
    }

    // An unknown format fails instead of offering some vocabulary, prototype keys included.
    assert.throws(() => getReasoningEffortOptions({ chat_completion_source: 'custom', custom_api_format: 'toString' }), /Unknown custom API format/);
});

test('Project levels (e.g. from a SillyTavern preset) map into a Custom / OpenCode format instead of being dropped', () => {
    for (const [settings, expected] of [
        [{ chat_completion_source: 'custom', custom_api_format: 'openai_compat', reasoning_effort: 'min' }, 'low'],
        [{ chat_completion_source: 'opencode', opencode_api_format: 'claude_messages', reasoning_effort: 'min' }, 'low'],
        [{ chat_completion_source: 'custom', custom_api_format: 'gemini_interactions', reasoning_effort: 'max' }, 'high'],
        [{ chat_completion_source: 'opencode', opencode_api_format: 'gemini', reasoning_effort: 'xhigh' }, 'high'],
        // A level the format offers itself is sent as is.
        [{ chat_completion_source: 'custom', custom_api_format: 'claude_messages', reasoning_effort: 'max' }, 'max'],
    ]) {
        const stored = structuredClone(settings);
        assert.equal(getEffectiveReasoningEffort(settings), expected, JSON.stringify(settings));
        assert.deepEqual(settings, stored);
    }
});
