import { chat_metadata, saveSettingsDebounced } from '../../script.js';
import { extension_settings, saveMetadataDebounced } from '../extensions.js';
import { convertValueType } from '../utils.js';
import { normalizeVariableValue } from './values.js';

/** Bind the same variable operations to live storage or an assembly's working values. */
function createVariableScope(getValues, onChange, scopeName) {
    const scope = {
        get(name, args = {}) {
            let value = getValues()[args.key ?? name];
            if (args.index !== undefined) {
                try {
                    value = JSON.parse(value);
                    const index = Number(args.index);
                    value = value[Number.isNaN(index) ? args.index : index];
                    if (typeof value === 'object') value = JSON.stringify(value);
                } catch {
                    // Preserve the existing indexed-read behavior for non-JSON values.
                }
            }
            return normalizeVariableValue(value);
        },
        set(name, value, args = {}) {
            if (!name) throw new Error('Variable name cannot be empty or undefined.');
            const values = getValues();
            if (args.index !== undefined) {
                try {
                    let container = JSON.parse(values[name] ?? 'null');
                    const index = Number(args.index);
                    const isKey = Number.isNaN(index);
                    container ??= isKey ? {} : [];
                    container[isKey ? args.index : index] = convertValueType(value, args.as);
                    values[name] = JSON.stringify(container);
                } catch {
                    // Preserve the existing indexed-write behavior for non-JSON values.
                }
            } else {
                values[name] = value;
            }
            onChange?.();
            return value;
        },
        add(name, value) {
            const currentValue = scope.get(name) || 0;
            try {
                const array = JSON.parse(currentValue);
                if (Array.isArray(array)) {
                    array.push(value);
                    scope.set(name, JSON.stringify(array));
                    return array;
                }
            } catch {
                // A non-array value is added numerically or concatenated below.
            }
            const increment = Number(value);
            if (isNaN(increment) || isNaN(Number(currentValue))) {
                const result = String(currentValue || '') + value;
                scope.set(name, result);
                return result;
            }
            const result = Number(currentValue) + increment;
            if (isNaN(result)) return '';
            scope.set(name, result);
            return result;
        },
        inc: name => scope.add(name, 1),
        dec: name => scope.add(name, -1),
        has: name => getValues()[name] !== undefined,
        del(name) {
            if (!scope.has(name)) {
                console.warn(`The ${scopeName} variable "${name}" does not exist.`);
                return '';
            }
            delete getValues()[name];
            onChange?.();
            return '';
        },
    };
    return scope;
}

// Resolve the canonical stores when an operation runs, including after chat switches.
export const liveVariables = {
    local: createVariableScope(() => chat_metadata.variables ??= {}, saveMetadataDebounced, 'local'),
    global: createVariableScope(() => extension_settings.variables.global, saveSettingsDebounced, 'global'),
};

/** Bind an assembly's already-detached raw variable values without persisting them. */
export function createMacroVariables(values) {
    return {
        local: createVariableScope(() => values.local, null, 'local'),
        global: createVariableScope(() => values.global, null, 'global'),
    };
}

/**
 * Returns built-in variable macros.
 * @returns {import('../macros.js').Macro[]}
 */
export function getVariableMacros(variables = liveVariables) {
    return [
        // Replace {{setvar::name::value}} with empty string and set the variable name to value
        { regex: /{{setvar::([^:]+)::([^}]*)}}/gi, replace: (_, name, value) => { variables.local.set(name.trim(), value); return ''; } },
        // Replace {{addvar::name::value}} with empty string and add value to the variable value
        { regex: /{{addvar::([^:]+)::([^}]+)}}/gi, replace: (_, name, value) => { variables.local.add(name.trim(), value); return ''; } },
        // Replace {{incvar::name}} with empty string and increment the variable name by 1
        { regex: /{{incvar::([^}]+)}}/gi, replace: (_, name) => variables.local.inc(name.trim()) },
        // Replace {{decvar::name}} with empty string and decrement the variable name by 1
        { regex: /{{decvar::([^}]+)}}/gi, replace: (_, name) => variables.local.dec(name.trim()) },
        // Replace {{getvar::name}} with the value of the variable name
        { regex: /{{getvar::([^}]+)}}/gi, replace: (_, name) => variables.local.get(name.trim()) },
        // Replace {{setglobalvar::name::value}} with empty string and set the global variable name to value
        { regex: /{{setglobalvar::([^:]+)::([^}]*)}}/gi, replace: (_, name, value) => { variables.global.set(name.trim(), value); return ''; } },
        // Replace {{addglobalvar::name::value}} with empty string and add value to the global variable value
        { regex: /{{addglobalvar::([^:]+)::([^}]+)}}/gi, replace: (_, name, value) => { variables.global.add(name.trim(), value); return ''; } },
        // Replace {{incglobalvar::name}} with empty string and increment the global variable name by 1
        { regex: /{{incglobalvar::([^}]+)}}/gi, replace: (_, name) => variables.global.inc(name.trim()) },
        // Replace {{decglobalvar::name}} with empty string and decrement the global variable name by 1
        { regex: /{{decglobalvar::([^}]+)}}/gi, replace: (_, name) => variables.global.dec(name.trim()) },
        // Replace {{getglobalvar::name}} with the value of the global variable name
        { regex: /{{getglobalvar::([^}]+)}}/gi, replace: (_, name) => variables.global.get(name.trim()) },
    ];
}
