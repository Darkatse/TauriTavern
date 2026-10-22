// @ts-check

/** Fill missing DTO fields without replacing saved settings with defaults. @param {any} defaults @param {any} saved */
function mergeDefaults(defaults, saved) {
    const result = { ...defaults, ...saved };
    for (const key of Object.keys(defaults)) {
        if (defaults[key] !== null && typeof defaults[key] === 'object' && !Array.isArray(defaults[key])) {
            result[key] = mergeDefaults(defaults[key], saved?.[key]);
        }
    }
    return result;
}

/** @param {import('./install.js').WebHostOptions['store']} store @param {() => Promise<any>} loadDefaults */
export function createSettings(store, loadDefaults) {
    /** Option fields ignore null; arrays and scalar values replace, objects update declared fields. @param {any} target @param {any} patch @param {any} shape */
    function applyOptions(target, patch, shape) {
        for (const key of Object.keys(shape)) {
            const value = patch[key];
            if (value == null) continue;
            if (shape[key] !== null && typeof shape[key] === 'object' && !Array.isArray(shape[key])) {
                applyOptions(target[key], value, shape[key]);
            } else target[key] = value;
        }
    }
    /** @param {any} initial */
    const read = initial => structuredClone(mergeDefaults(initial, store.load() ?? {}));
    return {
        async get() { return read(await loadDefaults()); },
        /** @param {any} patch */
        async update(patch) {
            patch = structuredClone(patch);
            const initial = await loadDefaults();
            const value = read(initial);
            const { updates, request_proxy, ...options } = patch;
            applyOptions(value, options, initial);
            // These two DTOs contain required fields rather than nested Option patches.
            if (request_proxy != null) value.request_proxy = request_proxy;
            if (updates != null) {
                if (updates.channel != null) value.updates.channel = updates.channel;
                value.updates.startup_popup.dismissed_release_token = updates.startup_popup.dismissed_release_token ?? null;
            }
            store.save(value);
            return structuredClone(value);
        },
    };
}
