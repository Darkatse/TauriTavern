/**
 * Shared live and explicit-context macro entry points.
 *
 * Exposes the MacroEngine / MacroRegistry singletons and provides a
 * single registerMacros() function that wires up all built-in macro
 * definition sets (core, env, state, chat, time, variables, instruct).
 */

// Engine singletons and enums
import { MacroEngine } from './engine/MacroEngine.js';
import { MacroRegistry, MacroCategory, MacroValueType } from './engine/MacroRegistry.js';
import { MacroLexer } from './engine/MacroLexer.js';
import { MacroParser } from './engine/MacroParser.js';
import { MacroCstWalker } from './engine/MacroCstWalker.js';
import { MacroEnvBuilder } from './engine/MacroEnvBuilder.js';
import { evaluateLegacyWithEnv } from '../macros.js';

// Macro definition groups
import { registerCoreMacros } from './definitions/core-macros.js';
import { registerEnvMacros } from './definitions/env-macros.js';
import { registerStateMacros } from './definitions/state-macros.js';
import { registerChatMacros } from './definitions/chat-macros.js';
import { registerTimeMacros } from './definitions/time-macros.js';
import { registerVariableMacros } from './definitions/variable-macros.js';
import { registerInstructMacros } from './definitions/instruct-macros.js';

// Re-export the category enum for external use
export { MacroCategory, MacroValueType };

// Re-export most-used jsdoc definitions
/** @typedef {import('./engine/MacroRegistry.js').MacroDefinitionOptions} MacroDefinitionOptions */
/** @typedef {import('./engine/MacroRegistry.js').MacroDefinition} MacroDefinition */
/** @typedef {import('./engine/MacroRegistry.js').MacroUnnamedArgDef} MacroUnnamedArgDef */
/** @typedef {import('./engine/MacroRegistry.js').MacroListSpec} MacroListSpec */
/** @typedef {import('./engine/MacroRegistry.js').MacroHandler} MacroHandler */
/** @typedef {import('./engine/MacroRegistry.js').MacroExecutionContext} MacroExecutionContext */

/** @typedef {import('chevrotain').CstNode} CstNode */
/** @typedef {import('./engine/MacroEnv.types.js').MacroEnv} MacroEnv */
/** @typedef {import('./engine/MacroEnv.types.js').MacroEnvNames} MacroEnvNames */
/** @typedef {import('./engine/MacroEnv.types.js').MacroEnvCharacter} MacroEnvCharacter */
/** @typedef {import('./engine/MacroEnv.types.js').MacroEnvSystem} MacroEnvSystem */
/** @typedef {import('./engine/MacroEnv.types.js').MacroEnvFunctions} MacroEnvFunctions */

/**
 * Capture reusable input data without evaluating any character templates.
 * @returns {import('./engine/MacroEnv.types.js').MacroContext}
 */
export function captureContext() {
    return MacroEnvBuilder.captureContext();
}

/**
 * Evaluate one top-level text; repeated calls share the caller's variable data.
 * @param {string} content
 * @param {import('./engine/MacroEnv.types.js').MacroContext} context
 * @param {import('./engine/MacroEnv.types.js').MacroEvaluationOptions} [options]
 * @returns {string}
 */
export function evaluateWithContext(content, context, options = {}) {
    if (typeof content !== 'string') throw new TypeError('Macro input must be a string');
    if (!content) return '';
    const env = MacroEnvBuilder.buildFromContext({ ...options, content }, context);
    return evaluateMacroEnv(content, env);
}

/** The parser choice belongs to the prepared input, not the current UI. */
export function evaluateMacroEnv(content, env) {
    if (env.engine === 'new') return MacroEngine.evaluate(content, env);
    if (env.engine === 'legacy') return evaluateLegacyWithEnv(content, env);
    throw new Error('macro.engine_invalid: Unknown macro engine');
}

export const macros = {
    captureContext,
    evaluateWithContext,
    // engine singletons
    engine: MacroEngine,
    registry: MacroRegistry,
    envBuilder: MacroEnvBuilder,
    lexer: MacroLexer,
    parser: MacroParser,
    cstWalker: MacroCstWalker,

    // enums
    category: MacroCategory,
    valueType: MacroValueType,

    // shorthand functions
    register: MacroRegistry.registerMacro.bind(MacroRegistry),
    registerAlias: MacroRegistry.registerMacroAlias.bind(MacroRegistry),
};

/**
 * Registers all built-in macros in a well-defined order.
 * Intended to be called once during app initialization.
 */
export function initRegisterMacros() {
    // Core utilities and generic helpers
    registerCoreMacros();

    // Env / character / system / extras
    registerEnvMacros();

    // Runtime state tracking (eventSource etc.)
    registerStateMacros();

    // Chat/history inspection macros
    registerChatMacros();

    // Time / date / durations
    registerTimeMacros();

    // Variable and instruct macros
    registerVariableMacros();
    registerInstructMacros();
}
