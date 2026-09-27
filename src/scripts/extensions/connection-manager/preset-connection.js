// @ts-check

/**
 * Which stored connection a Chat Completion preset keeps when it is saved while a
 * Connection Manager item owns the connection. Free of UI and settings imports, so
 * the upstream save paths (overwrite, save as, rename) can be exercised directly.
 * @param {Iterable<string>} connectionKeys Preset keys that describe the connection
 */
export function createPresetConnectionKeeper(connectionKeys) {
    const keys = [...connectionKeys];
    /** @type {{ newName: string, source: Record<string, unknown> } | null} */
    let rename = null;

    return {
        /**
         * An upstream rename first stores an empty placeholder under the new name and
         * then overwrites it from the live settings, so neither the placeholder nor the
         * live settings hold the renamed preset's connection: take it from the old entry
         * before it goes away.
         * @param {string} newName Name the preset is renamed to
         * @param {Record<string, unknown> | null} oldPreset Stored body of the preset being renamed
         */
        renaming(newName, oldPreset) {
            rename = { newName, source: structuredClone(oldPreset ?? {}) };
        },

        /**
         * Called for every preset save. While an item owns the connection, the body's
         * connection keys become the ones the preset already stores: the renamed
         * preset's, else the overwritten preset's, else (a new name) those of the loaded
         * preset it was saved from. A key the source does not store is dropped. A rename
         * capture only serves the save that directly follows it.
         * @param {object} save
         * @param {string} save.name Name being saved
         * @param {Record<string, unknown>} save.preset Body being saved; changed in place
         * @param {Record<string, unknown> | null} save.previous Stored preset this save overwrites
         * @param {Record<string, unknown> | null} save.loaded Stored body of the loaded preset
         * @param {boolean} save.owned Whether a selected item owns the connection
         */
        saving({ name, preset, previous, loaded, owned }) {
            const renamed = rename?.newName === name ? rename.source : null;
            rename = null;
            if (!owned) {
                return;
            }
            const source = renamed ?? previous ?? loaded;
            for (const key of keys) {
                if (source && Object.hasOwn(source, key)) {
                    preset[key] = structuredClone(source[key]);
                } else {
                    delete preset[key];
                }
            }
        },
    };
}
