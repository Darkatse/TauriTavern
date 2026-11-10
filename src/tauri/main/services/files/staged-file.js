// @ts-check

/**
 * A file in host staging: its extension is its format, its name only a label, and importing it
 * consumes it. Only host APIs create one.
 */
export class StagedFile {
    /** @param {string} path @param {string} name */
    constructor(path, name) {
        this.path = path;
        this.name = name;
        Object.freeze(this);
    }
}
