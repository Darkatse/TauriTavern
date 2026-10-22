// @ts-check

import { isDesktopHost } from '../../../scripts/util/host-identity.js';
import { createFileStagingService } from '../services/files/file-staging-service.js';
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

    return [...new Set(values.map((path) => requireNonEmptyString(path, 'Skill import path')))];
}

/**
 * @param {{
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 * }} deps
 */
function createSkillApi({
    safeInvoke,
}) {
    const staging = createFileStagingService({ safeInvoke });

    // Pickers and cleanup share one batch. First-party views hold this lease
    // through preview and installation, including in-flight cleanup.
    let importAcquired = false;
    function acquireImport() {
        if (importAcquired) throw new Error('skill.import_busy: another Skill import is still open');
        importAcquired = true;
        /** @type {Promise<void> | undefined} */
        let releasing;
        return () => {
            releasing ??= discardPickedImport().finally(() => { importAcquired = false; });
            return releasing;
        };
    }
    /** @type {Map<string, (() => Promise<void>) | null>} */
    const pendingPickedImports = new Map();

    function rememberPickedImport(input, cleanup) {
        pendingPickedImports.set(input.path, cleanup);
        return input;
    }

    async function discardPickedImport(input = null) {
        const selected = input == null ? null : normalizeSkillImportInput(input);
        const paths = selected === null
            ? [...pendingPickedImports.keys()]
            : selected.kind === 'archiveFile' ? [selected.path] : [];

        for (const path of paths) {
            const cleanup = pendingPickedImports.get(path);
            pendingPickedImports.delete(path);
            try {
                await safeInvoke('discard_skill_import_archive', { path });
            } catch (error) {
                console.warn('Failed to cleanup extracted Skill import archive:', error);
            }
            try {
                await cleanup?.();
            } catch (error) {
                console.warn('Failed to cleanup staged Skill import archive:', error);
            }
        }
    }

    async function discoverImports(options) {
        const input = normalizeSkillImportInput(options?.input);
        if (input.kind !== 'directory' && input.kind !== 'archiveFile') return [input];
        if (input.kind === 'archiveFile' && !pendingPickedImports.has(input.path)) {
            rememberPickedImport(input, null);
        }
        const discovered = await safeInvoke('discover_skill_imports', {
            input: toSkillImportCommandInput(input),
        });
        if (!Array.isArray(discovered) || discovered.length === 0) {
            throw new Error('Skill import discovery returned no Skills');
        }
        return discovered.map(normalizeSkillImportInput);
    }

    async function pickImportArchiveInputs(multiple) {
        await discardPickedImport();
        const files = await safeInvoke('pick_import_files', { kind: 'skill-archive', multiple });
        return files?.map(({ path }) => rememberPickedImport(
            { kind: 'archiveFile', path },
            () => staging.discardFile(path),
        )) ?? null;
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

    function isDirectoryImportAvailable() {
        return isDesktopHost();
    }

    async function pickImportDirectories() {
        if (!isDirectoryImportAvailable()) {
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
        return paths?.map((path) => ({ kind: 'directory', path })) ?? null;
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
        return safeInvoke('preview_skill_import', {
            input: toSkillImportCommandInput(input),
            ...(targetScope ? { targetScope } : {}),
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
        acquireImport,
        list,
        listFiles,
        pickImportArchive,
        pickImportArchives,
        pickImportDirectories,
        isDirectoryImportAvailable,
        discardPickedImport,
        discoverImports,
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
    });
}
