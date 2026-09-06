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

function normalizePickedImportPaths(value) {
    if (value === null || value === undefined) {
        return null;
    }

    const values = Array.isArray(value) ? value : [value];
    if (values.length === 0) {
        return null;
    }

    return values.map((path) => requireNonEmptyString(path, 'Skill import path'));
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 *   materializeAndroidSkillImportArchive?: (contentUri: string) => Promise<any>;
 *   removeTemporaryFile?: (filePath: string) => Promise<void>;
 * }} deps
 */
function createSkillApi({
    safeInvoke,
    materializeAndroidSkillImportArchive,
    removeTemporaryFile,
}) {
    /** @type {Map<string, { cleanup: () => Promise<void>; remainingRoots: Set<string> | null }>} */
    const pendingPickedImports = new Map();

    function pickedImportRef(input) {
        if (input === null || input === undefined) {
            return null;
        }
        try {
            const normalized = normalizeSkillImportInput(input);
            return normalized.kind === 'archiveFile'
                ? { path: normalized.path, skillRoot: normalized.skillRoot || '' }
                : null;
        } catch {
            return null;
        }
    }

    function rememberPickedImport(input, cleanup) {
        pendingPickedImports.set(input.path, { cleanup, remainingRoots: null });
        return input;
    }

    function trackDiscoveredImports(inputs) {
        for (const [path, pending] of pendingPickedImports) {
            pending.remainingRoots = new Set(inputs
                .filter(input => input.kind === 'archiveFile' && input.path === path)
                .map(input => input.skillRoot || ''));
        }
    }

    async function discardPickedImport(input = null) {
        const picked = pickedImportRef(input);
        const paths = input === null || input === undefined
            ? [...pendingPickedImports.keys()]
            : picked ? [picked.path] : [];

        for (const path of paths) {
            const pending = pendingPickedImports.get(path);
            if (!pending) {
                continue;
            }
            if (picked && pending.remainingRoots) {
                pending.remainingRoots.delete(picked.skillRoot);
                if (pending.remainingRoots.size > 0) continue;
            }
            pendingPickedImports.delete(path);
            try {
                await pending.cleanup();
            } catch (error) {
                console.warn('Failed to cleanup staged Skill import archive:', error);
            }
        }
    }

    async function discoverPickedImports(inputs) {
        try {
            const discovered = await safeInvoke('discover_skill_imports', {
                inputs: inputs.map(input => toSkillImportCommandInput(normalizeSkillImportInput(input))),
            });
            if (!Array.isArray(discovered) || discovered.length === 0) {
                throw new Error('Skill import discovery returned no Skills');
            }
            const normalized = discovered.map(normalizeSkillImportInput);
            trackDiscoveredImports(normalized);
            return normalized;
        } catch (error) {
            await discardPickedImport();
            throw error;
        }
    }

    async function stageAndroidSkillImportArchive(contentUri) {
        if (typeof materializeAndroidSkillImportArchive !== 'function') {
            throw new Error('Android Skill import staging is unavailable');
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

    async function pickAndroidSkillImportArchives(multiple) {
        const contentUris = normalizePickedImportPaths(await safeInvoke('plugin:dialog|open', {
            options: {
                multiple,
                directory: false,
                filters: [
                    {
                        name: 'Agent Skill Archive',
                        extensions: ['application/zip', 'application/x-zip-compressed', 'application/octet-stream'],
                    },
                ],
            },
        }));
        if (!contentUris) {
            return null;
        }

        try {
            const inputs = [];
            for (const contentUri of contentUris) {
                inputs.push(await stageAndroidSkillImportArchive(contentUri));
            }
            return inputs;
        } catch (error) {
            await discardPickedImport();
            throw error;
        }
    }

    async function pickIosSkillImportArchives(multiple) {
        if (typeof removeTemporaryFile !== 'function') {
            throw new Error('iOS Skill import cleanup is unavailable');
        }

        const result = await safeInvoke('ios_pick_skill_import_archives', { multiple });
        if (result?.cancelled) {
            return null;
        }

        if (!Array.isArray(result?.filePaths) || result.filePaths.length === 0) {
            throw new Error('iOS Skill import picker returned no files');
        }
        return result.filePaths.map((filePath) => {
            const path = requireNonEmptyString(filePath, 'iOS Skill import file path');
            return rememberPickedImport(
                { kind: 'archiveFile', path },
                async () => {
                    await removeTemporaryFile(path);
                },
            );
        });
    }

    async function pickImportArchiveInputs(multiple) {
        await discardPickedImport();

        let inputs;
        if (isAndroidRuntime()) {
            inputs = await pickAndroidSkillImportArchives(multiple);
        } else if (isIosRuntime()) {
            inputs = await pickIosSkillImportArchives(multiple);
        } else {
            const paths = normalizePickedImportPaths(await safeInvoke('plugin:dialog|open', {
                options: {
                    title: multiple ? 'Import Agent Skill Archives' : 'Import Agent Skill',
                    multiple,
                    directory: false,
                    filters: [
                        {
                            name: 'Agent Skill Archive',
                            extensions: ['zip', 'ttskill'],
                        },
                    ],
                },
            }));
            inputs = paths?.map((path) => ({ kind: 'archiveFile', path })) ?? null;
        }
        return inputs && multiple ? discoverPickedImports(inputs) : inputs;
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
        const inputs = await pickImportArchiveInputs(false);
        return inputs?.[0] ?? null;
    }

    async function pickImportArchives() {
        return pickImportArchiveInputs(true);
    }

    async function pickImportDirectories() {
        if (isAndroidRuntime() || isIosRuntime()) {
            throw new Error('Skill directory import is only available on desktop');
        }
        await discardPickedImport();

        const paths = normalizePickedImportPaths(await safeInvoke('plugin:dialog|open', {
            options: {
                title: 'Import Agent Skill Folders',
                multiple: true,
                directory: true,
                recursive: true,
            },
        }));
        return paths
            ? discoverPickedImports(paths.map((path) => ({ kind: 'directory', path })))
            : null;
    }

    async function downloadImport(options) {
        const request = requirePlainObject(options, 'skill import download request');
        const url = requireNonEmptyString(request.url, 'skill import URL');
        return normalizeSkillImportInput(await safeInvoke('download_skill_import_url', { url }));
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
            await discardPickedImport(request.input);
            throw error;
        }
    }

    async function installImport(request) {
        try {
            return await safeInvoke('install_skill_import', {
                request: normalizeSkillInstallRequest(request),
            });
        } finally {
            await discardPickedImport(request?.input);
        }
    }

    async function readFile(options) {
        const name = requireNonEmptyString(options?.name, 'skill name');
        const path = requireNonEmptyString(options?.path, 'skill file path');
        const startLine = normalizeOptionalNonNegativeInteger(options?.startLine, 'startLine');
        const lineCount = normalizeOptionalNonNegativeInteger(options?.lineCount, 'lineCount');
        const scope = normalizeSkillScope(options?.scope, 'scope');
        return safeInvoke('read_skill_file', {
            name,
            path,
            ...(scope ? { scope } : {}),
            ...(startLine == null ? {} : { startLine }),
            ...(lineCount == null ? {} : { lineCount }),
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
        pickImportArchives,
        pickImportDirectories,
        discardPickedImport,
        downloadImport,
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
        removeTemporaryFile: context.removeTemporaryFile,
    });
}
