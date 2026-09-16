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


test('api.skill imports shared iOS candidates and releases all sources at batch end', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const released = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                if (command === 'ios_pick_skill_import_archives') return { cancelled: false, filePaths: ['/cache/skills.zip', '/cache/broken.zip'] };
                if (command === 'discover_skill_imports') {
                    if (args.input.path.endsWith('broken.zip')) throw new Error('Invalid ZIP');
                    return ['one', 'two'].map(skill_root => ({ ...args.input, skill_root }));
                }
                if (command === 'install_skill_import') return { name: args.request.input.skill_root };
                if (command === 'discard_skill_import_archive') released.push(args.path);
                return {};
            },
            removeTemporaryFile: async path => cleanups.push(path),
        });
        const sources = await skill.pickImportArchives();
        const candidates = await skill.discoverImports({ input: sources[0] });
        await assert.rejects(skill.discoverImports({ input: sources[1] }), /Invalid ZIP/);
        const installed = [];
        for (const input of candidates) installed.push((await skill.installImport({ input })).name);
        assert.deepEqual(installed, ['one', 'two']);
        assert.deepEqual(released, []);
        assert.deepEqual(cleanups, []);
        await skill.discardPickedImport();
        assert.deepEqual(released, ['/cache/skills.zip', '/cache/broken.zip']);
        assert.deepEqual(cleanups, released);
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
