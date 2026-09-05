/** Preserve the value coercion used by local and global variable reads. */
export function normalizeVariableValue(value) {
    return (value?.trim?.() === '' || isNaN(Number(value))) ? (value || '') : Number(value);
}

/** Capture macro text separately from the original variable values exposed to scripts. */
export function snapshotVariableMacroValues(variables, normalizeResult = String) {
    return Object.fromEntries(Object.entries(variables).map(([scope, values]) => [
        scope,
        Object.fromEntries(Object.entries(values)
            .filter(([, value]) => value !== undefined)
            .map(([name, value]) => [name, normalizeResult(normalizeVariableValue(value))])),
    ]));
}
