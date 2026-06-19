'use strict';

function isPlainObject(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function isPromptOrderReference(value) {
    return isPlainObject(value) && typeof value.identifier === 'string' && value.identifier.length > 0;
}

export function isPromptDefinition(value) {
    return isPlainObject(value) && typeof value.identifier === 'string' && value.identifier.length > 0;
}

function removeNullEntries(array) {
    let removed = 0;
    for (let index = array.length - 1; index >= 0; index--) {
        if (array[index] === null) {
            array.splice(index, 1);
            removed++;
        }
    }
    return removed;
}

export function repairNullPromptManagerEntries(settings) {
    if (!isPlainObject(settings)) {
        return 0;
    }

    let removed = 0;

    if (Array.isArray(settings.prompts)) {
        removed += removeNullEntries(settings.prompts);
    }

    if (!Array.isArray(settings.prompt_order)) {
        return removed;
    }

    removed += removeNullEntries(settings.prompt_order);

    for (const promptOrder of settings.prompt_order) {
        if (isPlainObject(promptOrder) && Array.isArray(promptOrder.order)) {
            removed += removeNullEntries(promptOrder.order);
        }
    }

    return removed;
}

export function assertPromptOrderReferences(promptOrder, label = 'prompt_order') {
    if (!Array.isArray(promptOrder)) {
        throw new Error(`${label} must be an array`);
    }

    for (let index = 0; index < promptOrder.length; index++) {
        if (!isPromptOrderReference(promptOrder[index])) {
            throw new Error(`${label}[${index}] must be a prompt reference with an identifier`);
        }
    }
}

export function resolvePromptOrderFromDomIdentifiers(promptOrder, identifiers) {
    assertPromptOrderReferences(promptOrder, 'prompt_order');

    const idToObjectMap = new Map(promptOrder.map(prompt => [prompt.identifier, prompt]));
    return identifiers.map(identifier => {
        const entry = idToObjectMap.get(identifier);
        if (!entry) {
            throw new Error(`prompt_order is missing a reference for identifier: ${identifier}`);
        }
        return entry;
    });
}

export function isPromptManagerImportDataValid(importData) {
    const data = importData?.data;
    if (!isPlainObject(data) || !Array.isArray(data.prompts)) {
        return false;
    }

    if (!data.prompts.every(isPromptDefinition)) {
        return false;
    }

    if (data.prompt_order == null) {
        return true;
    }

    if (!Array.isArray(data.prompt_order)) {
        return false;
    }

    return data.prompt_order.every(isPromptOrderReference);
}
