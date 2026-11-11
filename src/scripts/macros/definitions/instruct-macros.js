import { MacroRegistry, MacroCategory } from '../engine/MacroRegistry.js';

/**
 * Registers instruct-mode related {{...}} macros (instruct* and system
 * prompt/context macros) in the MacroRegistry.
 */
export function registerInstructMacros() {
    /**
     * Helper to register macros that just expose a value from the environment instruct settings.
     * The first name is the primary, subsequent names become visible aliases.
     * @param {string[]} names - First is primary, rest are aliases.
     * @param {(env: import('../engine/MacroEnv.types.js').MacroEnv) => string} getValue
     * @param {(env: import('../engine/MacroEnv.types.js').MacroEnv) => boolean} isEnabled
     * @param {string} description
     * @param {string} [category=MacroCategory.PROMPTS]
     */
    function registerSimple(names, getValue, isEnabled, description, category = MacroCategory.PROMPTS) {
        const [primary, ...aliasNames] = names;
        const aliases = aliasNames.map(alias => ({ alias }));

        MacroRegistry.registerMacro(primary, {
            category,
            description,
            aliases: aliases.length > 0 ? aliases : undefined,
            handler: ({ env }) => (isEnabled(env) ? (getValue(env) ?? '') : ''),
        });
    }

    const instEnabled = env => !!env.settings.instruct.enabled;
    const sysEnabled = env => !!env.settings.sysprompt.enabled;

    // Instruct template macros
    registerSimple(['instructStoryStringPrefix'], env => env.settings.instruct.story_string_prefix, instEnabled, 'Instruct story string prefix.');
    registerSimple(['instructStoryStringSuffix'], env => env.settings.instruct.story_string_suffix, instEnabled, 'Instruct story string suffix.');

    registerSimple(['instructUserPrefix', 'instructInput'], env => env.settings.instruct.input_sequence, instEnabled, 'Instruct input / user prefix sequence.');
    registerSimple(['instructUserSuffix'], env => env.settings.instruct.input_suffix, instEnabled, 'Instruct input / user suffix sequence.');

    registerSimple(['instructAssistantPrefix', 'instructOutput'], env => env.settings.instruct.output_sequence, instEnabled, 'Instruct output / assistant prefix sequence.');
    registerSimple(['instructAssistantSuffix', 'instructSeparator'], env => env.settings.instruct.output_suffix, instEnabled, 'Instruct output / assistant suffix sequence.');

    registerSimple(['instructSystemPrefix'], env => env.settings.instruct.system_sequence, instEnabled, 'Instruct system prefix sequence.');
    registerSimple(['instructSystemSuffix'], env => env.settings.instruct.system_suffix, instEnabled, 'Instruct system suffix sequence.');

    registerSimple(['instructFirstAssistantPrefix', 'instructFirstOutputPrefix'], env => env.settings.instruct.first_output_sequence || env.settings.instruct.output_sequence, instEnabled, 'Instruct first assistant / output prefix sequence');
    registerSimple(['instructLastAssistantPrefix', 'instructLastOutputPrefix'], env => env.settings.instruct.last_output_sequence || env.settings.instruct.output_sequence, instEnabled, 'Instruct last assistant / output prefix sequence.');

    registerSimple(['instructStop'], env => env.settings.instruct.stop_sequence, instEnabled, 'Instruct stop sequence.');
    registerSimple(['instructUserFiller'], env => env.settings.instruct.user_alignment_message, instEnabled, 'Instruct user alignment filler.');
    registerSimple(['instructSystemInstructionPrefix'], env => env.settings.instruct.last_system_sequence, instEnabled, 'Instruct system instruction prefix sequence.');

    registerSimple(['instructFirstUserPrefix', 'instructFirstInput'], env => env.settings.instruct.first_input_sequence || env.settings.instruct.input_sequence, instEnabled, 'Instruct first user / input prefix sequence.');
    registerSimple(['instructLastUserPrefix', 'instructLastInput'], env => env.settings.instruct.last_input_sequence || env.settings.instruct.input_sequence, instEnabled, 'Instruct last user / input prefix sequence.');

    // System prompt macros
    registerSimple(['defaultSystemPrompt', 'instructSystem', 'instructSystemPrompt'], env => env.settings.sysprompt.content, sysEnabled, 'Default system prompt.');

    MacroRegistry.registerMacro('systemPrompt', {
        category: MacroCategory.PROMPTS,
        description: 'Active system prompt text (optionally overridden by character prompt)',
        handler: ({ env }) => {
            const isEnabled = !!env.settings.sysprompt.enabled;
            if (!isEnabled) return '';

            if (env.settings.prefer_character_prompt && env.character.charPrompt) {
                return env.character.charPrompt;
            }
            return env.settings.sysprompt.content ?? '';
        },
    });

    // Context template macros
    registerSimple(['exampleSeparator', 'chatSeparator'], env => env.settings.context.example_separator, () => true, 'Separator used between example chat blocks in text completion prompts.');
    registerSimple(['chatStart'], env => env.settings.context.chat_start, () => true, 'Chat start marker used in text completion prompts.');
}
