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

/** @param {import('./install.js').WebHostOptions['store']} store @param {typeof fetch} fetchResource */
export function createSettings(store, fetchResource) {
    /** @type {Promise<any> | undefined} */
    let defaults;
    return {
        async get() {
            defaults ??= fetchResource(new URL('./default-settings.json', import.meta.url)).then(async response => {
                if (!response.ok) throw new Error(`Cannot load Web Host settings defaults: ${response.status}`);
                return response.json();
            });
            return mergeDefaults(await defaults, store.load() ?? {});
        },
    };
}
