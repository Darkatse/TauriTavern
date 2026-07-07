// @ts-check

function characterCardApi() {
    return window.__TAURITAVERN__?.api?.characterCards;
}

export function isNativeCharacterCardPickerAvailable() {
    const api = characterCardApi();
    return typeof api?.isNativePickerAvailable === 'function' && api.isNativePickerAvailable();
}

export async function pickNativeCharacterCardFiles(options = {}) {
    const api = characterCardApi();
    if (typeof api?.pickFiles !== 'function') {
        throw new Error('Native character card picker is unavailable');
    }

    const files = await api.pickFiles(options);
    if (files === null) {
        return null;
    }
    if (!Array.isArray(files) || files.some(file => !(file instanceof File))) {
        throw new Error('Native character card picker returned an invalid file list');
    }
    return files;
}
