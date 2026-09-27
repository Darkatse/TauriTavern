import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createBrowserRuntime } from './runtime.mjs';

for (const experimental of [true, false]) {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    try {
        await startHost();
        await load('script.js');
        window.fetch = async input => {
            const pathname = new URL(String(input), 'http://localhost/').pathname;
            if (pathname.startsWith('/scripts/extensions/') && pathname.endsWith('.html')) {
                return new Response(readFileSync(new URL(`../../src${pathname}`, import.meta.url), 'utf8'));
            }
            throw new Error(`Unexpected request: ${pathname}`);
        };
        const script = getModule('script.js').namespace;
        const { power_user } = getModule('scripts/power-user.js').namespace;
        const { extension_settings } = getModule('scripts/extensions.js').namespace;
        const { macros } = getModule('scripts/macros/macro-system.js').namespace;
        power_user.experimental_macro_engine = experimental;
        getModule('scripts/macros.js').namespace.initMacros();
        const expressions = (await load('scripts/extensions/expressions/index.js')).namespace;
        await expressions.init();
        await (await load('scripts/extensions/memory/index.js')).namespace.init();
        getModule('scripts/authors-note.js').namespace.initAuthorsNote();

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
            assert.equal(evaluate('{{lastExpression::ELODIE}}|{{lastExpression::elodie.png}}|{{lastExpression::older-elodie.png}}|{{lastExpression::Other}}'), 'joy|joy|sadness|surprise');
        }

        expressions.lastExpression.ElodieSprites = 'anger';
        script.chat_metadata.note_prompt = 'Changed note';
        script.chat[0].extra.memory = 'Changed summary';
        script.setCharacterId(2);
        script.setCharacterName('Other');
        input.value = 'Changed input';
        assert.equal(evaluate('{{lastExpression}}|{{authorsNote}}|{{summary}}'), 'joy|Original note|Earlier summary');

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
            assert.equal(script.substituteParams(text, { replaceCharacterCard: false }),
                'Second character/Changed input|Changed note|Changed summary|joy|surprise||Later message/Later input|Later note|Later summary|#none|anger');
            const original = 'Second character/Original input|Original note|Earlier summary|joy|joy';
            assert.equal(evaluate(text), `${original}||${original}`);
        }
        console.log(`PASS: core and module macros preserve live reads and captured data (${experimental ? 'new' : 'legacy'} engine)`);
    } finally {
        await window.happyDOM.close();
    }
}
