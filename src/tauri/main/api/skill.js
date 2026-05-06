// @ts-check

/**
 * @param {unknown} value
 * @param {string} label
 */
function requireNonEmptyString(value, label) {
    const resolved = String(value || '').trim();
    if (!resolved) {
        throw new Error(`${label} is required`);
    }
    return resolved;
}

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {Record<string, any>}
 */
function requirePlainObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }

    return /** @type {Record<string, any>} */ (value);
}

/**
 * @param {unknown} value
 * @returns {Record<string, any>}
 */
function normalizeSource(value) {
    if (value === null || value === undefined) {
        return {};
    }

    return requirePlainObject(value, 'source');
}

/**
 * @param {unknown} value
 */
function normalizeEncoding(value) {
    const encoding = String(value || 'utf8').trim().toLowerCase();
    if (!['utf8', 'utf-8', 'base64'].includes(encoding)) {
        throw new Error(`Unsupported skill file encoding: ${encoding}`);
    }

    return encoding;
}

/**
 * @param {unknown} value
 * @param {string} label
 */
function normalizeOptionalNonNegativeInteger(value, label) {
    if (value === null || value === undefined) {
        return undefined;
    }

    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`${label} must be a non-negative integer`);
    }

    return number;
}

/**
 * @param {unknown} value
 */
function normalizeSkillInlineFile(value) {
    const file = requirePlainObject(value, 'skill inline file');
    /** @type {Record<string, any>} */
    const output = {
        path: requireNonEmptyString(file.path, 'skill file path'),
        encoding: normalizeEncoding(file.encoding),
        content: String(file.content ?? ''),
    };

    if (typeof file.content !== 'string') {
        throw new Error('skill file content must be a string');
    }

    const mediaType = String(file.mediaType || '').trim();
    if (mediaType) {
        output.mediaType = mediaType;
    }

    const sizeBytes = normalizeOptionalNonNegativeInteger(file.sizeBytes, 'sizeBytes');
    if (sizeBytes !== undefined) {
        output.sizeBytes = sizeBytes;
    }

    const sha256 = String(file.sha256 || '').trim();
    if (sha256) {
        output.sha256 = sha256;
    }

    return output;
}

/**
 * @param {unknown} value
 */
function normalizeSkillImportInput(value) {
    const input = requirePlainObject(value, 'skill import input');
    const kind = requireNonEmptyString(input.kind, 'skill import kind');

    if (kind === 'inlineFiles') {
        if (!Array.isArray(input.files) || input.files.length === 0) {
            throw new Error('inlineFiles skill import requires at least one file');
        }

        return {
            kind,
            files: input.files.map(normalizeSkillInlineFile),
            source: normalizeSource(input.source),
        };
    }

    if (kind === 'directory' || kind === 'archiveFile') {
        return {
            kind,
            path: requireNonEmptyString(input.path, 'skill import path'),
            source: normalizeSource(input.source),
        };
    }

    if (kind === 'archiveBase64') {
        const output = {
            kind,
            fileName: requireNonEmptyString(input.fileName, 'skill archive file name'),
            contentBase64: requireNonEmptyString(input.contentBase64, 'skill archive contentBase64'),
            source: normalizeSource(input.source),
        };
        const sha256 = String(input.sha256 || '').trim();
        if (sha256) {
            output.sha256 = sha256;
        }
        return output;
    }

    throw new Error(`Unsupported skill import kind: ${kind}`);
}

/**
 * @param {ReturnType<typeof normalizeSkillImportInput>} input
 */
function toSkillImportCommandInput(input) {
    if (input.kind !== 'archiveBase64') {
        return input;
    }

    /** @type {Record<string, any>} */
    const output = {
        kind: input.kind,
        file_name: input.fileName,
        content_base64: input.contentBase64,
        source: input.source,
    };
    if (input.sha256) {
        output.sha256 = input.sha256;
    }
    return output;
}

/**
 * @param {unknown} value
 */
function normalizeConflictStrategy(value) {
    if (value === null || value === undefined) {
        return undefined;
    }

    const strategy = requireNonEmptyString(value, 'conflictStrategy');
    if (strategy !== 'skip' && strategy !== 'replace') {
        throw new Error(`Unsupported skill conflict strategy: ${strategy}`);
    }

    return strategy;
}

/**
 * @param {unknown} value
 */
function normalizeSkillInstallRequest(value) {
    const request = requirePlainObject(value, 'skill install request');
    const input = normalizeSkillImportInput(request.input);
    /** @type {Record<string, any>} */
    const output = {
        input: toSkillImportCommandInput(input),
    };

    const conflictStrategy = normalizeConflictStrategy(request.conflictStrategy);
    if (conflictStrategy !== undefined) {
        output.conflictStrategy = conflictStrategy;
    }

    return output;
}

function normalizePickedImportArchivePath(value) {
    if (value === null || value === undefined) {
        return null;
    }

    const path = String(value).trim();
    if (!path) {
        return null;
    }

    return path;
}

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
function createSkillApi({ safeInvoke }) {
    async function list() {
        return safeInvoke('list_skills');
    }

    async function listFiles(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        return safeInvoke('list_skill_files', { name });
    }

    async function pickImportArchive() {
        const path = normalizePickedImportArchivePath(await safeInvoke('plugin:dialog|open', {
            options: {
                title: 'Import Agent Skill',
                multiple: false,
                directory: false,
                filters: [
                    {
                        name: 'Agent Skill Archive',
                        extensions: ['ttskill', 'zip'],
                    },
                ],
            },
        }));

        return path ? { kind: 'archiveFile', path } : null;
    }

    async function previewImport(input) {
        return safeInvoke('preview_skill_import', {
            input: toSkillImportCommandInput(normalizeSkillImportInput(input)),
        });
    }

    async function installImport(request) {
        return safeInvoke('install_skill_import', {
            request: normalizeSkillInstallRequest(request),
        });
    }

    async function readFile(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const path = requireNonEmptyString(options?.path, 'skill file path');
        const maxChars = normalizeOptionalNonNegativeInteger(options?.maxChars, 'maxChars');
        const startLine = normalizeOptionalNonNegativeInteger(options?.startLine, 'startLine');
        const lineCount = normalizeOptionalNonNegativeInteger(options?.lineCount, 'lineCount');
        const startChar = normalizeOptionalNonNegativeInteger(options?.startChar, 'startChar');
        return safeInvoke('read_skill_file', {
            name,
            path,
            maxChars,
            startLine,
            lineCount,
            startChar,
        });
    }

    async function exportSkill(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        return safeInvoke('export_skill', { name });
    }

    async function deleteSkill(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        return safeInvoke('delete_skill', { name });
    }

    return {
        list,
        listFiles,
        pickImportArchive,
        previewImport,
        installImport,
        readFile,
        export: exportSkill,
        exportSkill,
        delete: deleteSkill,
        deleteSkill,
    };
}

/**
 * @param {any} context
 */
export function installSkillApi(context) {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    const safeInvoke = context?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Tauri main context safeInvoke is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.skill = createSkillApi({ safeInvoke });
}
