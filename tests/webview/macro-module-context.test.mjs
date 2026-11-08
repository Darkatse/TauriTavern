import { expect, onTestFinished, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

for (const experimental of [true, false]) {
    test(`core and module macros preserve live reads and captured data (${experimental ? 'new' : 'legacy'} engine)`, async () => {
        const { window, load, startHost, close } = await createFrontend();
        onTestFinished(close);
        await startHost();
        await load('script.js');
        // Extension templates are the only requests the macros may make.
        const serve = window.fetch;
        window.fetch = (input, init) => {
            const { pathname } = new URL(String(input), window.location.href);
            return pathname.startsWith('/scripts/extensions/') && pathname.endsWith('.html')
                ? serve(input, init)
                : window.Promise.reject(new window.Error(`Unexpected request: ${pathname}`));
        };
        const script = (await load('script.js'));
        const { power_user } = (await load('scripts/power-user.js'));
        const { extension_settings } = (await load('scripts/extensions.js'));
        const { macros } = (await load('scripts/macros/macro-system.js'));
        power_user.experimental_macro_engine = experimental;
        (await load('scripts/macros.js')).initMacros();
        const expressions = (await load('scripts/extensions/expressions/index.js'));
        await expressions.init();
        await (await load('scripts/extensions/memory/index.js')).init();
        (await load('scripts/authors-note.js')).initAuthorsNote();

        script.characters.push(
            { name: 'Élodie', avatar: 'older-elodie.png', data: {} },
            { name: 'Élodie', avatar: 'elodie.png', data: {} },
            { name: 'Other', avatar: 'other.png', data: {} },
        );
        script.setCharacterId(1);
        script.setCharacterName('Élodie');
        script.chat.push(
            { name: 'Alias', original_avatar: 'elodie.png', mes: 'First character', extra: { memory: 'Earlier summary' } },
            { name: 'Other', original_avatar: 'other.png', mes: 'Second character' },
        );
        extension_settings.expressionOverrides.push({ name: 'elodie', path: 'ElodieSprites/dressed' });
        expressions.lastExpression.Élodie = 'sadness';
        expressions.lastExpression.ElodieSprites = 'joy';
        expressions.lastExpression.Other = 'surprise';
        script.chat_metadata.note_prompt = 'Original note';

        extension_settings.expressions.fallback_expression = 'joy';
        const input = window.document.querySelector('#send_textarea');
        input.value = 'Original input';
        const captured = macros.captureContext();
        const evaluate = text => macros.evaluateWithContext(text, captured);
        if (experimental) {
            expect(evaluate('{{lastExpression::ELODIE}}|{{lastExpression::elodie.png}}|{{lastExpression::older-elodie.png}}|{{lastExpression::Other}}')).toBe('joy|joy|sadness|surprise');
        }

        expressions.lastExpression.ElodieSprites = 'anger';
        script.chat_metadata.note_prompt = 'Changed note';
        script.chat[0].extra.memory = 'Changed summary';
        script.setCharacterId(2);
        script.setCharacterName('Other');
        input.value = 'Changed input';
        expect(evaluate('{{lastExpression}}|{{authorsNote}}|{{summary}}')).toBe('joy|Original note|Earlier summary');

        if (experimental) {
            macros.register('changeModuleState', {
                handler: () => {
                    script.chat.at(-1).mes = 'Later message';
                    input.value = 'Later input';
                    script.chat_metadata.note_prompt = 'Later note';
                    script.chat[0].extra.memory = 'Later summary';
                    extension_settings.expressions.fallback_expression = '#none';
                    expressions.lastExpression.Other = 'anger';
                    return '';
                },
            });
            const moduleText = '{{lastMessage}}/{{input}}|{{authorsNote}}|{{summary}}|{{defaultExpression}}|{{lastExpression}}';
            const text = `${moduleText}|{{changeModuleState}}|${moduleText}`;
            expect(script.substituteParams(text, { replaceCharacterCard: false })).toBe('Second character/Changed input|Changed note|Changed summary|joy|surprise||Later message/Later input|Later note|Later summary|#none|anger');
            const original = 'Second character/Original input|Original note|Earlier summary|joy|joy';
            expect(evaluate(text)).toBe(`${original}||${original}`);
        }
    });
}
