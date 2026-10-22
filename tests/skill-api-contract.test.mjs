import { installHostIdentity, HOSTS } from './helpers/host-identity.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

test.beforeEach(t => t.after(installHostIdentity()));

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Skill import ownership lasts through preview and cleanup, and release is idempotent', async () => {
    const cleanup = Promise.withResolvers();
    const started = Promise.withResolvers();
    let cleanupCount = 0;
    const { skill } = await installHarness({ safeInvoke: async command => {
        if (command === 'pick_import_files') return [{ path: '/tmp/owned-skill.zip', name: 'owned-skill.zip' }];
        if (command === 'discard_skill_import_archive') { cleanupCount++; started.resolve(); await cleanup.promise; }
    } });
    const release = skill.acquireImport();
    await skill.pickImportArchive();
    assert.throws(() => skill.acquireImport(), /skill.import_busy/);
    const releasing = release();
    await started.promise;
    const repeatedRelease = release();
    assert.throws(() => skill.acquireImport(), /skill.import_busy/);
    cleanup.resolve();
    await Promise.all([releasing, repeatedRelease]);
    assert.equal(cleanupCount, 1);
    const next = skill.acquireImport();
    await release();
    assert.throws(() => skill.acquireImport(), /skill.import_busy/);
    await next();
});

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

async function withHostIdentity(identity, callback) {
    const restore = installHostIdentity(identity);
    try {
        return await callback();
    } finally {
        restore();
    }
}


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


test('api.skill imports shared iOS candidates and releases all sources at batch end', async () => {
    await withHostIdentity(HOSTS.ios, async () => {
        const released = [];
        const cleanups = [];
        const { skill } = await installHarness({
            safeInvoke: async (command, args) => {
                if (command === 'pick_import_files') return ['skills.zip', 'broken.zip'].map(name => ({ path: `/cache/${name}`, name }));
                if (command === 'discover_skill_imports') {
                    if (args.input.path.endsWith('broken.zip')) throw new Error('Invalid ZIP');
                    return ['one', 'two'].map(skill_root => ({ ...args.input, skill_root }));
                }
                if (command === 'install_skill_import') return { name: args.request.input.skill_root };
                if (command === 'discard_skill_import_archive') released.push(args.path);
                if (command === 'stage_file_discard') cleanups.push(args.filePath);
                return {};
            },
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
