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


test('api.skill rejects non-string file writes', async () => {
    const { skill } = await installHarness();

    await assert.rejects(
        () => skill.writeFile({ name: 'test-skill', path: 'SKILL.md', content: null }),
        /skill file content must be a string/,
    );
});


test('api.skill recursively expands archives and Skill folders through desktop dialogs', async () => {
    const calls = [];
    const { skill } = await installHarness({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'discover_skill_imports') {
                return args.inputs[0].kind === 'archiveFile'
                    ? [
                        { ...args.inputs[0], skill_root: 'skills/one' },
                        { ...args.inputs[1], skill_root: 'nested/skills/two' },
                    ]
                    : [
                        { kind: 'directory', path: '/tmp/skill-one', source: {} },
                        { kind: 'directory', path: '/tmp/parent/nested/skill-two', source: {} },
                    ];
            }
            if (command === 'preview_skill_import') return {};
            return args.options.directory
                ? ['/tmp/parent']
                : ['/tmp/one.zip', '/tmp/two.ttskill'];
        },
    });

    const archiveInputs = await skill.pickImportArchives();
    assert.deepEqual(archiveInputs, [
        { kind: 'archiveFile', path: '/tmp/one.zip', skillRoot: 'skills/one', source: {} },
        { kind: 'archiveFile', path: '/tmp/two.ttskill', skillRoot: 'nested/skills/two', source: {} },
    ]);
    assert.deepEqual(await skill.pickImportDirectories(), [
        { kind: 'directory', path: '/tmp/skill-one', source: {} },
        { kind: 'directory', path: '/tmp/parent/nested/skill-two', source: {} },
    ]);
    await skill.previewImport({ input: archiveInputs[0] });
    assert.deepEqual(calls.map(({ command }) => command), [
        'plugin:dialog|open',
        'discover_skill_imports',
        'plugin:dialog|open',
        'discover_skill_imports',
        'preview_skill_import',
    ]);
    assert.equal(calls[0].args.options.multiple, true);
    assert.equal(calls[0].args.options.directory, false);
    assert.equal(calls[2].args.options.multiple, true);
    assert.equal(calls[2].args.options.directory, true);
    assert.equal(calls[2].args.options.recursive, true);
    assert.deepEqual(calls[4].args.input, {
        kind: 'archiveFile',
        path: '/tmp/one.zip',
        skill_root: 'skills/one',
        source: {},
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


test('api.skill keeps a shared iOS archive until all discovered Skills are consumed', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const calls = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                if (command === 'discover_skill_imports') {
                    return [
                        { ...args.inputs[0], skillRoot: 'skills/one' },
                        { ...args.inputs[0], skillRoot: 'skills/two' },
                    ];
                }
                if (command === 'install_skill_import') return {};
                return {
                    cancelled: false,
                    filePaths: ['/cache/skills.zip'],
                };
            },
            removeTemporaryFile: async (filePath) => cleanups.push(filePath),
        });

        assert.deepEqual(await skill.pickImportArchives(), [
            { kind: 'archiveFile', path: '/cache/skills.zip', skillRoot: 'skills/one', source: {} },
            { kind: 'archiveFile', path: '/cache/skills.zip', skillRoot: 'skills/two', source: {} },
        ]);
        assert.deepEqual(calls[0], {
            command: 'ios_pick_skill_import_archives',
            args: { multiple: true },
        });

        await skill.installImport({ input: { kind: 'archiveFile', path: '/cache/skills.zip', skillRoot: 'skills/one' } });
        assert.deepEqual(cleanups, []);
        await skill.installImport({ input: { kind: 'archiveFile', path: '/cache/skills.zip', skillRoot: 'skills/two' } });
        assert.deepEqual(cleanups, ['/cache/skills.zip']);
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
