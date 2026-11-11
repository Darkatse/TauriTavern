// @ts-check

/** Convert the ST settings projection into complete Persona records. @param {any} settings */
export function personaRecords(settings) {
    const { personas = {}, persona_descriptions = {} } = settings.power_user ?? {};
    return Object.fromEntries([...new Set([...Object.keys(personas), ...Object.keys(persona_descriptions)])]
        .map(id => [id, {
            ...(Object.hasOwn(personas, id) ? { name: personas[id] } : {}),
            ...(Object.hasOwn(persona_descriptions, id) ? { description: persona_descriptions[id] } : {}),
        }]));
}

/** Convert Persona records back to the ST settings projection. @param {Record<string, any>} records */
export function personaProjection(records) {
    /** @type {Record<string, any>} */
    const personas = {};
    /** @type {Record<string, any>} */
    const persona_descriptions = {};
    for (const [id, data] of Object.entries(records)) {
        if (data.name !== undefined) personas[id] = data.name;
        if (data.description !== undefined) persona_descriptions[id] = data.description;
    }
    return { personas, persona_descriptions };
}
