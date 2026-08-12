import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function installHarness(overrides = {}) {
    const calls = [];
    globalThis.window = {
        __TAURITAVERN__: { api: {} },
    };

    const { installSkillApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/skill.js')));
    installSkillApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        },
        ...overrides,
    });

    return {
        calls,
        skill: globalThis.window.__TAURITAVERN__.api.skill,
    };
}

async function withNavigatorUserAgent(userAgent, callback) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent },
        configurable: true,
    });

    try {
        return await callback();
    } finally {
        if (descriptor) {
            Object.defineProperty(globalThis, 'navigator', descriptor);
        } else {
            delete globalThis.navigator;
        }
    }
}

test('api.skill installs and forwards normalized import DTOs', async () => {
    const { calls, skill } = await installHarness();

    assert.ok(skill);
    await skill.previewImport({
        input: {
            kind: 'inlineFiles',
            files: [
                {
                    path: 'SKILL.md',
                    content: '---\nname: test-skill\ndescription: Use in tests.\n---\n',
                },
            ],
            source: { kind: 'preset', label: 'Test preset' },
        },
        targetScope: { kind: 'preset', apiId: 'openai', name: 'Creative' },
    });

    assert.equal(calls[0].command, 'preview_skill_import');
    assert.deepEqual(calls[0].args.input, {
        kind: 'inlineFiles',
        files: [
            {
                path: 'SKILL.md',
                encoding: 'utf8',
                content: '---\nname: test-skill\ndescription: Use in tests.\n---\n',
            },
        ],
        source: { kind: 'preset', label: 'Test preset' },
    });
    assert.deepEqual(calls[0].args.targetScope, { kind: 'preset', apiId: 'openai', name: 'Creative' });
});

test('api.skill forwards install conflict strategy without implicit replace', async () => {
    const { calls, skill } = await installHarness();
    const input = {
        kind: 'inlineFiles',
        files: [{ path: 'SKILL.md', content: '---\nname: test-skill\ndescription: Use in tests.\n---\n' }],
    };

    await skill.installImport({ input });
    await skill.installImport({ input, conflictStrategy: 'replace' });

    assert.deepEqual(calls[0].args.request, {
        input: {
            kind: 'inlineFiles',
            files: [{ path: 'SKILL.md', encoding: 'utf8', content: '---\nname: test-skill\ndescription: Use in tests.\n---\n' }],
            source: {},
        },
    });
    assert.equal(calls[1].args.request.conflictStrategy, 'replace');
});

test('api.skill maps archiveBase64 command fields to Rust DTO names', async () => {
    const { calls, skill } = await installHarness();
    const input = {
        kind: 'archiveBase64',
        fileName: 'embedded-skill.zip',
        contentBase64: 'UEsDBAo=',
        sha256: 'abc123',
        source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
    };

    await skill.previewImport({ input });
    await skill.installImport({ input, conflictStrategy: 'replace' });

    assert.deepEqual(calls[0].args.input, {
        kind: 'archiveBase64',
        file_name: 'embedded-skill.zip',
        content_base64: 'UEsDBAo=',
        sha256: 'abc123',
        source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
    });
    assert.deepEqual(calls[1].args.request, {
        input: {
            kind: 'archiveBase64',
            file_name: 'embedded-skill.zip',
            content_base64: 'UEsDBAo=',
            sha256: 'abc123',
            source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
        },
        conflictStrategy: 'replace',
    });
});

test('api.skill downloads remote SKILL.md through host command', async () => {
    const calls = [];
    const { skill } = await installHarness({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return {
                kind: 'inlineFiles',
                files: [{
                    path: 'SKILL.md',
                    content: '---\nname: downloaded\ndescription: Use in tests.\n---\n',
                }],
                source: { kind: 'url', id: 'https://example.com/SKILL.md', label: 'https://example.com/SKILL.md' },
            };
        },
    });

    const input = await skill.downloadImport({
        url: 'https://example.com/SKILL.md',
    });

    assert.equal(calls[0].command, 'download_skill_import_url');
    assert.deepEqual(calls[0].args, { url: 'https://example.com/SKILL.md' });
    assert.deepEqual(input, {
        kind: 'inlineFiles',
        files: [{
            path: 'SKILL.md',
            encoding: 'utf8',
            content: '---\nname: downloaded\ndescription: Use in tests.\n---\n',
        }],
        source: { kind: 'url', id: 'https://example.com/SKILL.md', label: 'https://example.com/SKILL.md' },
    });
});

test('api.skill lists installed skill files by skill name', async () => {
    const { calls, skill } = await installHarness();

    await skill.listFiles({ scope: { kind: 'profile', profileId: 'writer' }, name: 'test-skill' });

    assert.equal(calls[0].command, 'list_skill_files');
    assert.deepEqual(calls[0].args, {
        name: 'test-skill',
        scope: { kind: 'profile', profileId: 'writer' },
    });
});

test('api.skill deletes installed skills by skill name', async () => {
    const { calls, skill } = await installHarness();

    await skill.delete({ scope: { kind: 'global' }, name: 'test-skill' });

    assert.equal(calls[0].command, 'delete_skill');
    assert.deepEqual(calls[0].args, { name: 'test-skill', scope: { kind: 'global' } });
});

test('api.skill forwards scope filters and move requests', async () => {
    const { calls, skill } = await installHarness();

    await skill.list({ scope: { kind: 'all' } });
    await skill.move({
        name: 'test-skill',
        fromScope: { kind: 'global' },
        toScope: { kind: 'character', characterId: 'Aurelia' },
        conflictStrategy: 'replace',
    });

    assert.deepEqual(calls[0], {
        command: 'list_skills',
        args: { scope: { kind: 'all' } },
    });
    assert.deepEqual(calls[1], {
        command: 'move_skill',
        args: {
            request: {
                name: 'test-skill',
                fromScope: { kind: 'global' },
                toScope: { kind: 'character', characterId: 'Aurelia' },
                conflictStrategy: 'replace',
            },
        },
    });
});

test('api.skill writes text files with optimistic hash', async () => {
    const { calls, skill } = await installHarness();

    await skill.writeFile({
        scope: { kind: 'global' },
        name: 'test-skill',
        path: 'SKILL.md',
        content: 'updated',
        expectedSha256: 'abc123',
    });

    assert.deepEqual(calls[0], {
        command: 'write_skill_file',
        args: {
            name: 'test-skill',
            path: 'SKILL.md',
            content: 'updated',
            scope: { kind: 'global' },
            expectedSha256: 'abc123',
        },
    });
});

test('api.skill reads and exports files in explicit scopes', async () => {
    const { calls, skill } = await installHarness();

    await skill.readFile({
        scope: { kind: 'preset', apiId: 'openai', name: 'Creative' },
        name: 'test-skill',
        path: 'SKILL.md',
        startLine: 12,
        lineCount: 30,
    });
    await skill.export({
        scope: { kind: 'character', characterId: 'Aurelia' },
        name: 'test-skill',
    });

    assert.deepEqual(calls[0], {
        command: 'read_skill_file',
        args: {
            name: 'test-skill',
            path: 'SKILL.md',
            scope: { kind: 'preset', apiId: 'openai', name: 'Creative' },
            startLine: 12,
            lineCount: 30,
        },
    });
    assert.deepEqual(calls[1], {
        command: 'export_skill',
        args: {
            name: 'test-skill',
            scope: { kind: 'character', characterId: 'Aurelia' },
        },
    });
});

test('api.skill rejects non-string file writes', async () => {
    const { skill } = await installHarness();

    await assert.rejects(
        () => skill.writeFile({ name: 'test-skill', path: 'SKILL.md', content: null }),
        /skill file content must be a string/,
    );
});

test('api.skill picks import archives through the host dialog', async () => {
    const calls = [];
    const { skill } = await installHarness({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return '/tmp/test-skill.zip';
        },
    });

    const input = await skill.pickImportArchive();

    assert.deepEqual(input, { kind: 'archiveFile', path: '/tmp/test-skill.zip' });
    assert.equal(calls[0].command, 'plugin:dialog|open');
    assert.equal(calls[0].args.options.multiple, false);
    assert.equal(calls[0].args.options.directory, false);
    assert.deepEqual(calls[0].args.options.filters, [
        { name: 'Agent Skill Archive', extensions: ['zip', 'ttskill'] },
    ]);
});

test('api.skill picks multiple archives and Skill folders through desktop dialogs', async () => {
    const calls = [];
    const { skill } = await installHarness({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return args.options.directory
                ? ['/tmp/skill-one', '/tmp/skill-two']
                : ['/tmp/one.zip', '/tmp/two.ttskill'];
        },
    });

    assert.deepEqual(await skill.pickImportArchives(), [
        { kind: 'archiveFile', path: '/tmp/one.zip' },
        { kind: 'archiveFile', path: '/tmp/two.ttskill' },
    ]);
    assert.deepEqual(await skill.pickImportDirectories(), [
        { kind: 'directory', path: '/tmp/skill-one' },
        { kind: 'directory', path: '/tmp/skill-two' },
    ]);
    assert.deepEqual(calls.map(({ command }) => command), ['plugin:dialog|open', 'plugin:dialog|open']);
    assert.equal(calls[0].args.options.multiple, true);
    assert.equal(calls[0].args.options.directory, false);
    assert.equal(calls[1].args.options.multiple, true);
    assert.equal(calls[1].args.options.directory, true);
    assert.equal(calls[1].args.options.recursive, true);
});

test('api.skill stages Android picked content URIs as archive files', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (Linux; Android 15)', async () => {
        const calls = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return 'content://picked-skill';
            },
            materializeAndroidSkillImportArchive: async (contentUri) => {
                assert.equal(contentUri, 'content://picked-skill');
                return {
                    filePath: '/cache/tauritavern-skill-import-staging/picked.zip',
                    cleanup: async () => cleanups.push('picked.zip'),
                };
            },
        });

        const input = await skill.pickImportArchive();
        assert.deepEqual(input, {
            kind: 'archiveFile',
            path: '/cache/tauritavern-skill-import-staging/picked.zip',
        });
        assert.equal(calls[0].command, 'plugin:dialog|open');
        assert.equal(calls[0].args.options.multiple, false);

        await skill.discardPickedImport(input);
        assert.deepEqual(cleanups, ['picked.zip']);
    });
});

test('api.skill keeps Android archive cleanup failures from blocking the next pick', async (t) => {
    const originalConsoleWarn = console.warn;
    console.warn = () => {};
    t.after(() => {
        console.warn = originalConsoleWarn;
    });

    await withNavigatorUserAgent('Mozilla/5.0 (Linux; Android 15)', async () => {
        const cleanups = [];
        let pickerCalls = 0;
        const { skill } = await installHarness({
            safeInvoke: async (command) => {
                if (command !== 'plugin:dialog|open') {
                    return { action: 'installed', name: 'one' };
                }
                pickerCalls += 1;
                return pickerCalls === 1
                    ? ['content://one', 'content://two']
                    : ['content://three'];
            },
            materializeAndroidSkillImportArchive: async (contentUri) => ({
                filePath: `/cache/${contentUri.split('//')[1]}.zip`,
                cleanup: async () => {
                    cleanups.push(contentUri);
                    if (contentUri === 'content://two') {
                        throw new Error('cleanup failed');
                    }
                },
            }),
        });

        const inputs = await skill.pickImportArchives();
        assert.deepEqual(inputs, [
            { kind: 'archiveFile', path: '/cache/one.zip' },
            { kind: 'archiveFile', path: '/cache/two.zip' },
        ]);

        await skill.installImport({ input: inputs[0] });
        assert.deepEqual(cleanups, ['content://one']);

        assert.deepEqual(await skill.pickImportArchives(), [
            { kind: 'archiveFile', path: '/cache/three.zip' },
        ]);
        assert.equal(pickerCalls, 2);
        assert.deepEqual(cleanups, ['content://one', 'content://two']);

        await skill.discardPickedImport();
        assert.deepEqual(cleanups, ['content://one', 'content://two', 'content://three']);
    });
});

test('api.skill cleans staged Android archives when a later selection cannot be staged', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (Linux; Android 15)', async () => {
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async () => ['content://one', 'content://broken'],
            materializeAndroidSkillImportArchive: async (contentUri) => {
                if (contentUri.endsWith('broken')) {
                    throw new Error('staging failed');
                }
                return {
                    filePath: '/cache/one.zip',
                    cleanup: async () => cleanups.push(contentUri),
                };
            },
        });

        await assert.rejects(() => skill.pickImportArchives(), /staging failed/);
        assert.deepEqual(cleanups, ['content://one']);
    });
});

test('api.skill stages iOS picked files through the native command', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const calls = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return {
                    cancelled: false,
                    filePaths: ['/cache/tauritavern-skill-import-staging/picked.zip'],
                };
            },
            removeTemporaryFile: async (filePath) => cleanups.push(filePath),
        });

        const input = await skill.pickImportArchive();
        assert.deepEqual(input, {
            kind: 'archiveFile',
            path: '/cache/tauritavern-skill-import-staging/picked.zip',
        });
        assert.deepEqual(calls[0], {
            command: 'ios_pick_skill_import_archives',
            args: { multiple: false },
        });

        await skill.discardPickedImport(input);
        assert.deepEqual(cleanups, ['/cache/tauritavern-skill-import-staging/picked.zip']);
    });
});

test('api.skill stages and cleans multiple iOS archives', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const calls = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return {
                    cancelled: false,
                    filePaths: ['/cache/one.zip', '/cache/two.zip'],
                };
            },
            removeTemporaryFile: async (filePath) => cleanups.push(filePath),
        });

        assert.deepEqual(await skill.pickImportArchives(), [
            { kind: 'archiveFile', path: '/cache/one.zip' },
            { kind: 'archiveFile', path: '/cache/two.zip' },
        ]);
        assert.deepEqual(calls[0], {
            command: 'ios_pick_skill_import_archives',
            args: { multiple: true },
        });

        await skill.discardPickedImport();
        assert.deepEqual(cleanups, ['/cache/one.zip', '/cache/two.zip']);
        await assert.rejects(() => skill.pickImportDirectories(), /only available on desktop/);
    });
});

test('api.skill fails fast on unsupported import shapes', async () => {
    const { skill } = await installHarness();

    await assert.rejects(
        () => skill.previewImport({ input: { kind: 'base64Zip', content: 'abc' } }),
        /Unsupported skill import kind/,
    );
    await assert.rejects(
        () => skill.previewImport({ input: { kind: 'inlineFiles', files: [] } }),
        /requires at least one file/,
    );
    await assert.rejects(
        () => skill.installImport({ input: { kind: 'directory', path: '/tmp/skill' }, conflictStrategy: 'merge' }),
        /Unsupported skill conflict strategy/,
    );
    await assert.rejects(
        () => skill.listFiles({ name: '' }),
        /skill name is required/,
    );
    await assert.rejects(
        () => skill.delete({ name: '' }),
        /skill name is required/,
    );
});
