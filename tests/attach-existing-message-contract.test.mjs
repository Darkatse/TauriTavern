import test from 'node:test';
import assert from 'node:assert/strict';

import {
    INJECTION_POSITION,
    applyAttachedPromptsToMessages,
    applyPromptManagerOverrides,
    getPromptInjectionGroups,
    getPromptInjectionPosition,
    getRelativePromptById,
} from '../src/scripts/prompt-injections.js';

function cloneMessages(messages) {
    return messages.map(message => ({ ...message }));
}

test('attach-existing appends/prepends to role-filtered chronological chat messages', () => {
    const messages = cloneMessages([
        { role: 'assistant', content: 'assistant newest' },
        { role: 'user', content: 'user newest' },
        { role: 'assistant', content: 'assistant oldest' },
        { role: 'user', content: 'user oldest' },
    ]);

    const applied = applyAttachedPromptsToMessages([
        { identifier: 'first-user-end', content: 'append marker', attach_role: 'user', attach_index: 1, attach_side: 'end' },
        { identifier: 'last-assistant-start', content: 'prepend marker', attach_role: 'assistant', attach_index: -1, attach_side: 'start' },
    ], messages, { warn: assert.fail });

    assert.equal(applied, 2);
    assert.equal(messages[3].content, 'user oldest\n\nappend marker');
    assert.equal(messages[0].content, 'prepend marker\n\nassistant newest');
});

test('attach-existing clamps out-of-range indexes to the farthest message in that direction', () => {
    const messages = cloneMessages([
        { role: 'user', content: 'newest' },
        { role: 'user', content: 'middle' },
        { role: 'user', content: 'oldest' },
    ]);

    applyAttachedPromptsToMessages([
        { identifier: 'positive-overflow', content: 'positive clamp', attach_role: 'user', attach_index: 99, attach_side: 'end' },
        { identifier: 'negative-overflow', content: 'negative clamp', attach_role: 'user', attach_index: -99, attach_side: 'start' },
    ], messages, { warn: assert.fail });

    assert.equal(messages[0].content, 'newest\n\npositive clamp');
    assert.equal(messages[2].content, 'negative clamp\n\noldest');
});

test('attach-existing warns and skips invalid targets', () => {
    const warnings = [];
    const messages = cloneMessages([{ role: 'user', content: 'hello' }]);

    const applied = applyAttachedPromptsToMessages([
        { identifier: 'bad-role', content: 'x', attach_role: 'developer', attach_index: 1, attach_side: 'end' },
        { identifier: 'bad-index', content: 'x', attach_role: 'user', attach_index: 1.5, attach_side: 'end' },
        { identifier: 'missing-role', content: 'x', attach_role: 'assistant', attach_index: 1, attach_side: 'end' },
    ], messages, { warn: warning => warnings.push(warning) });

    assert.equal(applied, 0);
    assert.equal(messages[0].content, 'hello');
    assert.equal(warnings.length, 3);
    assert.match(warnings[0], /invalid target role/);
    assert.match(warnings[1], /invalid message index/);
    assert.match(warnings[2], /found no existing assistant messages/);
});

test('prompt injection grouping treats missing position as relative and excludes attached prompts from standalone groups', () => {
    const prompts = {
        collection: [
            { identifier: 'legacy-user', system_prompt: false },
            { identifier: 'attached-user', system_prompt: false, injection_position: INJECTION_POSITION.ATTACH_EXISTING },
            { identifier: 'absolute-user', system_prompt: false, injection_position: INJECTION_POSITION.ABSOLUTE },
            { identifier: 'legacy-system', system_prompt: true },
        ],
    };

    const groups = getPromptInjectionGroups(prompts);

    assert.equal(getPromptInjectionPosition(prompts.collection[0]), INJECTION_POSITION.RELATIVE);
    assert.deepEqual(groups.userRelativePromptIds, ['legacy-user']);
    assert.deepEqual(groups.absolutePrompts.map(prompt => prompt.identifier), ['absolute-user']);
    assert.deepEqual(groups.attachedPrompts.map(prompt => prompt.identifier), ['attached-user']);
});

test('relative prompt lookup excludes attached control prompts from standalone insertion', () => {
    const relativePrompt = { identifier: 'quietPrompt', injection_position: INJECTION_POSITION.RELATIVE };
    const attachedPrompt = { identifier: 'groupNudge', injection_position: INJECTION_POSITION.ATTACH_EXISTING };
    const prompts = {
        get(identifier) {
            return identifier === 'quietPrompt' ? relativePrompt : attachedPrompt;
        },
    };

    assert.equal(getRelativePromptById(prompts, 'quietPrompt'), relativePrompt);
    assert.equal(getRelativePromptById(prompts, 'groupNudge'), null);
});

test('prompt manager overrides include attach-existing target fields', () => {
    const generatedPrompt = { identifier: 'summary', role: 'system' };
    const markerPrompt = {
        role: 'assistant',
        injection_position: INJECTION_POSITION.ATTACH_EXISTING,
        attach_role: 'assistant',
        attach_index: -1,
        attach_side: 'start',
    };

    applyPromptManagerOverrides(generatedPrompt, markerPrompt);

    assert.equal(generatedPrompt.role, 'assistant');
    assert.equal(generatedPrompt.injection_position, INJECTION_POSITION.ATTACH_EXISTING);
    assert.equal(generatedPrompt.attach_role, 'assistant');
    assert.equal(generatedPrompt.attach_index, -1);
    assert.equal(generatedPrompt.attach_side, 'start');
});
