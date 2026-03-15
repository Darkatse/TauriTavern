// @ts-check

/**
 * Register SillyTavern's stscript language for highlight.js.
 *
 * This logic was previously embedded inside `SlashCommandParser.registerLanguage()`,
 * but moving it here lets us lazy-load highlight.js without forcing it onto the
 * Shell/Core startup path.
 *
 * @param {any} hljs
 */
export function registerStscriptLanguage(hljs) {
    // NUMBER mode is copied from highlightjs's own implementation for JavaScript
    // https://tc39.es/ecma262/#sec-literals-numeric-literals
    const decimalDigits = '[0-9](_?[0-9])*';
    const frac = `\\.(${decimalDigits})`;
    // DecimalIntegerLiteral, including Annex B NonOctalDecimalIntegerLiteral
    // https://tc39.es/ecma262/#sec-additional-syntax-numeric-literals
    const decimalInteger = '0|[1-9](_?[0-9])*|0[0-7]*[89][0-9]*';
    const NUMBER = {
        className: 'number',
        variants: [
            // DecimalLiteral
            { begin: `(\\b(${decimalInteger})((${frac})|\\.)?|(${frac}))` +
    `[eE][+-]?(${decimalDigits})\\b` },
            { begin: `\\b(${decimalInteger})\\b((${frac})\\b|\\.)?|(${frac})\\b` },

            // DecimalBigIntegerLiteral
            { begin: '\\b(0|[1-9](_?[0-9])*)n\\b' },

            // NonDecimalIntegerLiteral
            { begin: '\\b0[xX][0-9a-fA-F](_?[0-9a-fA-F])*n?\\b' },
            { begin: '\\b0[bB][0-1](_?[0-1])*n?\\b' },
            { begin: '\\b0[oO][0-7](_?[0-7])*n?\\b' },

            // LegacyOctalIntegerLiteral (does not include underscore separators)
            // https://tc39.es/ecma262/#sec-additional-syntax-numeric-literals
            { begin: '\\b0[0-7]+n?\\b' },
        ],
        relevance: 0,
    };

    function getQuotedRunRegex() {
        try {
            return new RegExp('(\".+?(?<!\\\\)\")|((?:[^\\s\\|\"]|\"[^\"]*\")*)(\\||$|\\s)');
        } catch {
            // fallback for browsers that don't support lookbehind
            return /(\".+?\")|(\\S+?)(\\||$|\\s)/;
        }
    }

    const BLOCK_COMMENT = {
        scope: 'comment',
        begin: /\/\*/,
        end: /\*\|/,
        contains: [],
    };
    const COMMENT = {
        scope: 'comment',
        begin: /\/[/#]/,
        end: /\||$|:}/,
        contains: [],
    };
    const ABORT = {
        begin: /\/(abort|breakpoint)/,
        beginScope: 'abort',
        end: /\||$|(?=:})/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const IMPORT = {
        scope: 'command',
        begin: /\/(import)/,
        beginScope: 'keyword',
        end: /\||$|(?=:})/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const BREAK = {
        scope: 'command',
        begin: /\/(break)/,
        beginScope: 'keyword',
        end: /\||$|(?=:})/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const LET = {
        begin: [
            /\/(let|var)\s+/,
        ],
        beginScope: {
            1: 'variable',
        },
        end: /\||$|:}/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const SETVAR = {
        begin: /\/(setvar|setglobalvar)\s+/,
        beginScope: 'variable',
        end: /\||$|:}/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const GETVAR = {
        begin: /\/(getvar|getglobalvar)\s+/,
        beginScope: 'variable',
        end: /\||$|:}/,
        excludeEnd: false,
        returnEnd: true,
        contains: [],
    };
    const RUN = {
        match: [
            /\/:/,
            getQuotedRunRegex(),
            /\||$|(?=:})/,
        ],
        className: {
            1: 'variable.language',
            2: 'title.function.invoke',
        },
        contains: [], // defined later
    };
    const COMMAND = {
        scope: 'command',
        begin: /\/\S+/,
        beginScope: 'title.function',
        end: /\||$|(?=:})/,
        excludeEnd: false,
        returnEnd: true,
        contains: [], // defined later
    };
    const CLOSURE = {
        scope: 'closure',
        begin: /{:/,
        end: /:}(\(\))?/,
        beginScope: 'punctuation',
        endScope: 'punctuation',
        contains: [], // defined later
    };
    const NAMED_ARG = {
        scope: 'property',
        begin: /\w+=/,
        end: '',
    };
    const MACRO = {
        scope: 'variable',
        begin: /{{/,
        end: /}}/,
    };
    const PIPEBREAK = {
        beginScope: 'pipebreak',
        begin: /\|\|/,
        end: '',
    };
    const PIPE = {
        beginScope: 'pipe',
        begin: /\|/,
        end: '',
    };
    BLOCK_COMMENT.contains.push(
        BLOCK_COMMENT,
    );
    RUN.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        hljs.QUOTE_STRING_MODE,
        NUMBER,
        MACRO,
        CLOSURE,
    );
    IMPORT.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    BREAK.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    LET.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    SETVAR.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    GETVAR.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        hljs.QUOTE_STRING_MODE,
        NUMBER,
        MACRO,
        CLOSURE,
    );
    ABORT.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    COMMAND.contains.push(
        hljs.BACKSLASH_ESCAPE,
        NAMED_ARG,
        NUMBER,
        MACRO,
        CLOSURE,
        hljs.QUOTE_STRING_MODE,
    );
    CLOSURE.contains.push(
        hljs.BACKSLASH_ESCAPE,
        BLOCK_COMMENT,
        COMMENT,
        ABORT,
        IMPORT,
        BREAK,
        NAMED_ARG,
        NUMBER,
        MACRO,
        RUN,
        LET,
        GETVAR,
        SETVAR,
        COMMAND,
        'self',
        hljs.QUOTE_STRING_MODE,
        PIPEBREAK,
        PIPE,
    );
    hljs.registerLanguage('stscript', () => ({
        case_insensitive: false,
        keywords: [],
        contains: [
            hljs.BACKSLASH_ESCAPE,
            BLOCK_COMMENT,
            COMMENT,
            ABORT,
            IMPORT,
            BREAK,
            RUN,
            LET,
            GETVAR,
            SETVAR,
            COMMAND,
            CLOSURE,
            PIPEBREAK,
            PIPE,
        ],
    }));
}

