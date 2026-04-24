export const UNNAMED_PERSONA = '[Unnamed Persona]';

export function isPersonaDescriptorMeaningful(descriptor, { defaultPosition, defaultDepth, defaultRole }) {
    if (typeof descriptor === 'string') {
        return descriptor.trim().length > 0;
    }

    if (!descriptor || typeof descriptor !== 'object') {
        return false;
    }

    const description = typeof descriptor.description === 'string' ? descriptor.description.trim() : '';
    const title = typeof descriptor.title === 'string' ? descriptor.title.trim() : '';
    const lorebook = typeof descriptor.lorebook === 'string' ? descriptor.lorebook.trim() : '';
    const connections = descriptor.connections;
    const hasConnections = Array.isArray(connections) ? connections.length > 0 : Boolean(connections);

    const position = descriptor.position ?? defaultPosition;
    const depth = descriptor.depth ?? defaultDepth;
    const role = descriptor.role ?? defaultRole;

    return description.length > 0
        || title.length > 0
        || lorebook.length > 0
        || hasConnections
        || position !== defaultPosition
        || depth !== defaultDepth
        || role !== defaultRole;
}

export function isPlaceholderPersona({ name, descriptor }, defaults) {
    return name === UNNAMED_PERSONA
        && !isPersonaDescriptorMeaningful(descriptor, defaults);
}

function normalizePersonaDescriptor(descriptor, defaults) {
    if (typeof descriptor === 'string') {
        return {
            description: descriptor,
            position: defaults.defaultPosition,
            depth: defaults.defaultDepth,
            role: defaults.defaultRole,
            lorebook: '',
            title: '',
        };
    }

    return descriptor;
}

export function restorePersonasFromBackup(target, backup, defaults) {
    const warnings = [];
    const restoredPersonas = new Set();

    // Merge personas with existing ones
    for (const [key, value] of Object.entries(backup.personas)) {
        if (key in target.personas) {
            if (isPlaceholderPersona({
                name: target.personas[key],
                descriptor: target.persona_descriptions?.[key],
            }, defaults)
                && typeof value === 'string'
                && value.trim()
                && value !== UNNAMED_PERSONA
            ) {
                target.personas[key] = value;
                restoredPersonas.add(key);
                continue;
            }

            warnings.push(`Persona "${key}" (${value}) already exists, skipping`);
            continue;
        }

        target.personas[key] = value;
        restoredPersonas.add(key);
    }

    // Merge persona descriptions with existing ones
    for (const [key, value] of Object.entries(backup.persona_descriptions)) {
        const normalizedDescriptor = normalizePersonaDescriptor(value, defaults);
        if (key in target.persona_descriptions) {
            const existingDescriptor = normalizePersonaDescriptor(target.persona_descriptions[key], defaults);
            if (existingDescriptor !== target.persona_descriptions[key]) {
                target.persona_descriptions[key] = existingDescriptor;
            }

            if (!isPersonaDescriptorMeaningful(existingDescriptor, defaults)
                && isPersonaDescriptorMeaningful(normalizedDescriptor, defaults)
            ) {
                target.persona_descriptions[key] = normalizedDescriptor;
                continue;
            }

            warnings.push(`Persona description for "${key}" (${target.personas[key]}) already exists, skipping`);
            continue;
        }

        if (!target.personas[key]) {
            warnings.push(`Persona for "${key}" does not exist, skipping`);
            continue;
        }

        target.persona_descriptions[key] = normalizedDescriptor;
    }

    if (backup.default_persona) {
        if (backup.default_persona in target.personas) {
            target.default_persona = backup.default_persona;
        } else {
            warnings.push(`Default persona "${backup.default_persona}" does not exist, skipping`);
        }
    }

    return { warnings, restoredPersonas };
}
