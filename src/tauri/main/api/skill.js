// @ts-check

import { isAndroidRuntime, isIosRuntime } from '../../../scripts/util/mobile-runtime.js';
import {
    normalizeOptionalNonNegativeInteger,
    normalizeSkillImportInput,
    normalizeSkillInstallRequest,
    normalizeSkillMoveRequest,
    normalizeSkillScope,
    normalizeSkillScopeFilter,
    normalizeSkillScopeRetargetRequest,
    requireNonEmptyString,
    requirePlainObject,
    toSkillImportCommandInput,
} from './skill-normalizers.js';

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

function isAndroidPickerCancel(error) {
    return String(error?.message || error || '').trim() === 'Import archive selection cancelled';
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   materializeAndroidSkillImportArchive?: (contentUri: string) => Promise<any>;
 *   pickAndroidImportArchive?: () => Promise<string>;
 *   removeTemporaryFile?: (filePath: string) => Promise<void>;
 * }} deps
 */
function createSkillApi({
    safeInvoke,
    materializeAndroidSkillImportArchive,
    pickAndroidImportArchive,
    removeTemporaryFile,
}) {
    /** @type {{ path: string; cleanup: () => Promise<void> } | null} */
    let pendingPickedImport = null;

    function pickedImportPath(input) {
        if (input === null || input === undefined) {
            return null;
        }
        try {
            const normalized = normalizeSkillImportInput(input);
            return normalized.kind === 'archiveFile' ? normalized.path : null;
        } catch {
            return null;
        }
    }

    function rememberPickedImport(input, cleanup) {
        pendingPickedImport = {
            path: input.path,
            cleanup,
        };
        return input;
    }

    async function discardPickedImport(input = null, { throwOnError = true } = {}) {
        if (!pendingPickedImport) {
            return;
        }
        const path = pickedImportPath(input);
        if (input !== null && input !== undefined && path !== pendingPickedImport.path) {
            return;
        }

        const current = pendingPickedImport;
        pendingPickedImport = null;
        try {
            await current.cleanup();
        } catch (error) {
            if (throwOnError) {
                throw error;
            }
            console.warn('Failed to cleanup staged Skill import archive:', error);
        }
    }

    async function pickAndroidSkillImportArchive() {
        if (typeof pickAndroidImportArchive !== 'function') {
            throw new Error('Android import picker is unavailable');
        }
        if (typeof materializeAndroidSkillImportArchive !== 'function') {
            throw new Error('Android Skill import staging is unavailable');
        }

        let contentUri;
        try {
            contentUri = await pickAndroidImportArchive();
        } catch (error) {
            if (isAndroidPickerCancel(error)) {
                return null;
            }
            throw error;
        }

        const fileInfo = await materializeAndroidSkillImportArchive(contentUri);
        if (!fileInfo?.filePath) {
            const reason = fileInfo?.error ? `: ${fileInfo.error}` : '';
            throw new Error(`Unable to stage Android Skill import archive${reason}`);
        }
        if (typeof fileInfo.cleanup !== 'function') {
            throw new Error('Android Skill import cleanup is unavailable');
        }

        return rememberPickedImport(
            { kind: 'archiveFile', path: fileInfo.filePath },
            async () => {
                await fileInfo.cleanup();
            },
        );
    }

    async function pickIosSkillImportArchive() {
        if (typeof removeTemporaryFile !== 'function') {
            throw new Error('iOS Skill import cleanup is unavailable');
        }

        const result = await safeInvoke('ios_pick_skill_import_archive');
        if (result?.cancelled) {
            return null;
        }

        const path = requireNonEmptyString(result?.filePath ?? result?.file_path, 'iOS Skill import file path');
        return rememberPickedImport(
            { kind: 'archiveFile', path },
            async () => {
                await removeTemporaryFile(path);
            },
        );
    }

    async function list(options = {}) {
        const request = requirePlainObject(options, 'skill list options');
        const scope = normalizeSkillScopeFilter(request.scope ?? request.filter, 'scope');
        return scope ? safeInvoke('list_skills', { scope }) : safeInvoke('list_skills');
    }

    async function listFiles(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const scope = normalizeSkillScope(options?.scope, 'scope');
        return safeInvoke('list_skill_files', {
            name,
            ...(scope ? { scope } : {}),
        });
    }

    async function pickImportArchive() {
        await discardPickedImport();

        if (isAndroidRuntime()) {
            return pickAndroidSkillImportArchive();
        }

        if (isIosRuntime()) {
            return pickIosSkillImportArchive();
        }

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

    async function previewImport(options) {
        const request = requirePlainObject(options, 'skill import preview request');
        const input = normalizeSkillImportInput(request.input);
        const targetScope = normalizeSkillScope(request.targetScope ?? request.target_scope, 'targetScope');
        try {
            return await safeInvoke('preview_skill_import', {
                input: toSkillImportCommandInput(input),
                ...(targetScope ? { targetScope } : {}),
            });
        } catch (error) {
            await discardPickedImport(request.input, { throwOnError: false });
            throw error;
        }
    }

    async function installImport(request) {
        try {
            return await safeInvoke('install_skill_import', {
                request: normalizeSkillInstallRequest(request),
            });
        } finally {
            await discardPickedImport(request?.input, { throwOnError: false });
        }
    }

    async function readFile(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const path = requireNonEmptyString(options?.path, 'skill file path');
        const maxChars = normalizeOptionalNonNegativeInteger(options?.maxChars, 'maxChars');
        const startLine = normalizeOptionalNonNegativeInteger(options?.startLine, 'startLine');
        const lineCount = normalizeOptionalNonNegativeInteger(options?.lineCount, 'lineCount');
        const startChar = normalizeOptionalNonNegativeInteger(options?.startChar, 'startChar');
        const scope = normalizeSkillScope(options?.scope, 'scope');
        return safeInvoke('read_skill_file', {
            name,
            path,
            ...(scope ? { scope } : {}),
            maxChars,
            startLine,
            lineCount,
            startChar,
        });
    }

    async function writeFile(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const path = requireNonEmptyString(options?.path, 'skill file path');
        if (typeof options?.content !== 'string') {
            throw new Error('skill file content must be a string');
        }
        const scope = normalizeSkillScope(options?.scope, 'scope');
        const expectedSha256 = String(options?.expectedSha256 ?? options?.expected_sha256 ?? '').trim();
        return safeInvoke('write_skill_file', {
            name,
            path,
            content: options.content,
            ...(scope ? { scope } : {}),
            ...(expectedSha256 ? { expectedSha256 } : {}),
        });
    }

    async function exportSkill(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const scope = normalizeSkillScope(options?.scope, 'scope');
        return safeInvoke('export_skill', {
            name,
            ...(scope ? { scope } : {}),
        });
    }

    async function deleteSkill(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const scope = normalizeSkillScope(options?.scope, 'scope');
        return safeInvoke('delete_skill', {
            name,
            ...(scope ? { scope } : {}),
        });
    }

    async function move(request) {
        return safeInvoke('move_skill', {
            request: normalizeSkillMoveRequest(request),
        });
    }

    async function retargetScope(request) {
        return safeInvoke('retarget_skill_scope', {
            request: normalizeSkillScopeRetargetRequest(request),
        });
    }

    return {
        list,
        listFiles,
        pickImportArchive,
        discardPickedImport,
        previewImport,
        installImport,
        readFile,
        writeFile,
        export: exportSkill,
        delete: deleteSkill,
        move,
        retargetScope,
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

    hostAbi.api.skill = createSkillApi({
        safeInvoke,
        materializeAndroidSkillImportArchive: context.materializeAndroidSkillImportArchive,
        pickAndroidImportArchive: context.pickAndroidImportArchive,
        removeTemporaryFile: context.removeTemporaryFile,
    });
}
