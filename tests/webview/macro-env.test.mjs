import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { load, startHost } = await createFrontend();

await startHost();
await load('script.js');
const { macros, initRegisterMacros } = (await load('scripts/macros/macro-system.js'));
const { MacrosParser } = (await load('scripts/macros.js'));
const { power_user } = (await load('scripts/power-user.js'));
const app = (await load('script.js'));
power_user.experimental_macro_engine = true;
initRegisterMacros();

test('named and shorthand macros share detached variable operations', () => {
    const context = macros.captureContext();
    context.variables = { local: { score: '2', items: '["first"]' }, global: { code: '007' } };
    const text = '{{setvar::score::3}}{{.score += 4}}{{incvar::score}}|{{getglobalvar::code}}'
        + '|{{setvarkey::items::1::second}}{{getvarkey::items::1}}';
    expect(macros.evaluateWithContext(text, context)).toBe('8|7|second');

    const original = app.chat_metadata.variables;
    try {
        app.chat_metadata.variables = { current: '41' };
        expect(app.substituteParams('{{getvar::current}}|{{replaceVariables}}|{{getvar::current}}', {
            replaceCharacterCard: false,
            dynamicMacros: { replaceVariables: () => { app.chat_metadata.variables = { current: '42' }; return ''; } },
        })).toBe('41||42');
    } finally {
        app.chat_metadata.variables = original;
    }
});

test('dynamic definitions govern delayed arguments and scoped arity before evaluation', () => {
    const context = macros.captureContext();
    context.variables.local.counter = 0;
    const hold = { unnamedArgs: 1, delayArgResolution: true, handler: () => 'held' };
    for (const [text, dynamicMacros] of [
        ['{{hold::{{incvar::counter}}}}', { hold }],
        // random normally takes a list and cannot accept scoped content.
        ['{{random}}{{incvar::counter}}{{/random}}', { random: hold }],
        ['{{if::true}}{{random}}{{incvar::counter}}{{/random}}{{/if}}', { random: hold }],
    ]) {
        expect(macros.evaluateWithContext(text, context, { dynamicMacros })).toBe('held');
        expect(context.variables.local.counter).toBe(0);
    }
});

test('legacy card fields retain their eager variable evaluation order', () => {
    const context = macros.captureContext();
    context.engine = 'legacy';
    context.variables = { local: { value: 'before', greetings: 0 }, global: {} };
    context.character.persona = '{{setvar::value::from-persona}}';
    context.character.scenario = '{{getvar::value}}';
    context.character.firstMessage = '{{incvar::greetings}}';
    context.character.alternateGreetings = ['{{incvar::greetings}}'];
    expect(macros.evaluateWithContext('{{scenario}}', context)).toBe('from-persona');
    expect(context.variables.local.greetings).toBe(2);
});

test('legacy phases keep their ordering and pass the explicit environment after nonce', () => {
    power_user.experimental_macro_engine = false;
    const context = macros.captureContext();
    context.names.char = 'Frozen';
    context.variables.local.value = 'before';
    context.character.charPrompt = 'card';
    const text = '{{legacyenvprobe}} {{legacyenvprobe}} {{getvar::value}}{{setvar::value::after}} {{charPrompt}}';
    const nonces = [];
    const callback = (nonce, suppliedEnv) => {
        nonces.push(nonce);
        return suppliedEnv.names.char;
    };
    MacrosParser.registerMacro('legacyenvprobe', callback);
    try {
        expect(macros.evaluateWithContext(text, context, { dynamicMacros: { CHARPROMPT: 'override' } })).toBe('Frozen Frozen after override');
        expect(typeof nonces[0]).toBe('string');
        expect(nonces[0]).toBe(nonces[1]);
        MacrosParser.unregisterMacro('legacyenvprobe');
        power_user.experimental_macro_engine = true;
        MacrosParser.registerMacro('legacyenvprobe', callback);
        context.engine = 'new';
        expect(macros.evaluateWithContext('{{legacyenvprobe}}', context)).toBe('Frozen');
    } finally {
        MacrosParser.unregisterMacro('legacyenvprobe');
        power_user.experimental_macro_engine = true;
    }
});
test("group name overrides retain each engine's participant rules", () => {
    const context = macros.captureContext();
    context.names.user = 'User';
    context.settings.isGroup = true;
    context.settings.groupNames = ['Alice', 'Bob', 'Muted'];
    context.settings.groupNamesNotMuted = ['Alice', 'Bob'];
    context.engine = 'new';
    expect(macros.evaluateWithContext('{{notChar}}', context, { name2Override: 'Bob' })).toBe('Alice');
    context.engine = 'legacy';
    expect(macros.evaluateWithContext('{{notChar}}|{{group}}', context,
        { name2Override: 'Bob', groupOverride: 'Explicit group' })).toBe('Alice, Muted, User|Explicit group');
});

test('direct new-engine callers keep their engine when the UI uses legacy', () => {
    const previous = power_user.experimental_macro_engine;
    power_user.experimental_macro_engine = false;
    try {
        const text = '{{notChar}}';
        const options = { name1Override: 'User', name2Override: 'Char', groupOverride: 'Group', replaceCharacterCard: false };
        expect(app.substituteParams(text, options)).toBe('User');
        const env = macros.envBuilder.buildFromRawEnv({ content: text, ...options });
        expect(macros.engine.evaluate(text, env)).toBe('Group');
        const captured = macros.captureContext();
        expect(macros.evaluateWithContext(text, captured, options)).toBe('User');
    } finally {
        power_user.experimental_macro_engine = previous;
    }
});

test('explicit evaluations retain captured names and reset per-text fields', () => {
    const previousPersona = power_user.persona_description;
    const previousVariables = app.chat_metadata.variables;
    let nickname = 'Captured';
    macros.envBuilder.registerProvider(env => {
        env.names.char = nickname;
    });
    try {
        power_user.persona_description = '{{incvar::frameCounter}}';
        app.chat_metadata.variables = { frameCounter: 0 };
        const context = macros.captureContext();
        nickname = 'Different live chat';

        const text = '{{char}}: {{persona}} {{persona}}|{{original}}/{{original}}';
        expect(macros.evaluateWithContext(text, context, { original: 'first' })).toBe('Captured: 1 1|first/');
        expect(macros.evaluateWithContext(text, context, { original: 'second' })).toBe('Captured: 2 2|second/');
        context.character.description = '{{char}}';
        expect(macros.evaluateWithContext('{{char}}|{{description}}', context, { name2Override: 'Other' }), "An outer name override does not replace the card's own default name").toBe('Other|Captured');
    } finally {
        power_user.persona_description = previousPersona;
        app.chat_metadata.variables = previousVariables;
    }
});

