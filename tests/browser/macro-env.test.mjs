import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

test('macro evaluation environments', async (context) => {
    const { window, getModule, load, startHost } = createBrowserRuntime();

    try {
        await startHost();
        await load('script.js');
        const { macros, initRegisterMacros } = getModule('scripts/macros/macro-system.js').namespace;
        const { MacrosParser } = getModule('scripts/macros.js').namespace;
        const { power_user } = getModule('scripts/power-user.js').namespace;
        const app = getModule('script.js').namespace;
        power_user.experimental_macro_engine = true;
        initRegisterMacros();

        await context.test('named and shorthand macros share detached variable operations', () => {
            const context = macros.captureContext();
            context.variables = { local: { score: '2', items: '["first"]' }, global: { code: '007' } };
            const text = '{{setvar::score::3}}{{.score += 4}}{{incvar::score}}|{{getglobalvar::code}}'
                + '|{{setvarkey::items::1::second}}{{getvarkey::items::1}}';
            assert.equal(macros.evaluateWithContext(text, context), '8|7|second');

            const original = app.chat_metadata.variables;
            try {
                app.chat_metadata.variables = { current: '41' };
                assert.equal(app.substituteParams('{{getvar::current}}|{{replaceVariables}}|{{getvar::current}}', {
                    replaceCharacterCard: false,
                    dynamicMacros: { replaceVariables: () => { app.chat_metadata.variables = { current: '42' }; return ''; } },
                }), '41||42');
            } finally {
                app.chat_metadata.variables = original;
            }
        });

        await context.test('dynamic definitions govern delayed arguments and scoped arity before evaluation', () => {
            const context = macros.captureContext();
            context.variables.local.counter = 0;
            const hold = { unnamedArgs: 1, delayArgResolution: true, handler: () => 'held' };
            for (const [text, dynamicMacros] of [
                ['{{hold::{{incvar::counter}}}}', { hold }],
                // random normally takes a list and cannot accept scoped content.
                ['{{random}}{{incvar::counter}}{{/random}}', { random: hold }],
                ['{{if::true}}{{random}}{{incvar::counter}}{{/random}}{{/if}}', { random: hold }],
            ]) {
                assert.equal(macros.evaluateWithContext(text, context, { dynamicMacros }), 'held');
                assert.equal(context.variables.local.counter, 0);
            }
        });

        await context.test('legacy card fields retain their eager variable evaluation order', () => {
            const context = macros.captureContext();
            context.engine = 'legacy';
            context.variables = { local: { value: 'before', greetings: 0 }, global: {} };
            context.character.persona = '{{setvar::value::from-persona}}';
            context.character.scenario = '{{getvar::value}}';
            context.character.firstMessage = '{{incvar::greetings}}';
            context.character.alternateGreetings = ['{{incvar::greetings}}'];
            assert.equal(macros.evaluateWithContext('{{scenario}}', context), 'from-persona');
            assert.equal(context.variables.local.greetings, 2);
        });

        await context.test('legacy phases keep their ordering and pass the explicit environment after nonce', () => {
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
                assert.equal(macros.evaluateWithContext(text, context, { dynamicMacros: { CHARPROMPT: 'override' } }), 'Frozen Frozen after override');
                assert.equal(typeof nonces[0], 'string');
                assert.equal(nonces[0], nonces[1]);
                MacrosParser.unregisterMacro('legacyenvprobe');
                power_user.experimental_macro_engine = true;
                MacrosParser.registerMacro('legacyenvprobe', callback);
                context.engine = 'new';
                assert.equal(macros.evaluateWithContext('{{legacyenvprobe}}', context), 'Frozen');
            } finally {
                MacrosParser.unregisterMacro('legacyenvprobe');
                power_user.experimental_macro_engine = true;
            }
        });
        await context.test("group name overrides retain each engine's participant rules", () => {
            const context = macros.captureContext();
            context.names.user = 'User';
            context.settings.isGroup = true;
            context.settings.groupNames = ['Alice', 'Bob', 'Muted'];
            context.settings.groupNamesNotMuted = ['Alice', 'Bob'];
            context.engine = 'new';
            assert.equal(macros.evaluateWithContext('{{notChar}}', context, { name2Override: 'Bob' }), 'Alice');
            context.engine = 'legacy';
            assert.equal(macros.evaluateWithContext('{{notChar}}|{{group}}', context,
                { name2Override: 'Bob', groupOverride: 'Explicit group' }), 'Alice, Muted, User|Explicit group');
        });

        await context.test('direct new-engine callers keep their engine when the UI uses legacy', () => {
            const previous = power_user.experimental_macro_engine;
            power_user.experimental_macro_engine = false;
            try {
                const text = '{{notChar}}';
                const options = { name1Override: 'User', name2Override: 'Char', groupOverride: 'Group', replaceCharacterCard: false };
                assert.equal(app.substituteParams(text, options), 'User');
                const env = macros.envBuilder.buildFromRawEnv({ content: text, ...options });
                assert.equal(macros.engine.evaluate(text, env), 'Group');
                const captured = macros.captureContext();
                assert.equal(macros.evaluateWithContext(text, captured, options), 'User');
            } finally {
                power_user.experimental_macro_engine = previous;
            }
        });

        await context.test('explicit evaluations retain captured names and reset per-text fields', () => {
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
                assert.equal(macros.evaluateWithContext(text, context, { original: 'first' }), 'Captured: 1 1|first/');
                assert.equal(macros.evaluateWithContext(text, context, { original: 'second' }), 'Captured: 2 2|second/');
                context.character.description = '{{char}}';
                assert.equal(macros.evaluateWithContext('{{char}}|{{description}}', context, { name2Override: 'Other' }),
                    'Other|Captured', "An outer name override does not replace the card's own default name");
            } finally {
                power_user.persona_description = previousPersona;
                app.chat_metadata.variables = previousVariables;
            }
        });

    } finally {
        await window.happyDOM.close();
    }
});
