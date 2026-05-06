import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function installHarness() {
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
    });

    return {
        calls,
        skill: globalThis.window.__TAURITAVERN__.api.skill,
    };
}

test('api.skill installs and forwards normalized import DTOs', async () => {
    const { calls, skill } = await installHarness();

    assert.ok(skill);
    await skill.previewImport({
        kind: 'inlineFiles',
        files: [
            {
                path: 'SKILL.md',
                content: '---\nname: test-skill\ndescription: Use in tests.\n---\n',
            },
        ],
        source: { kind: 'preset', label: 'Test preset' },
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
        fileName: 'embedded-skill.ttskill',
        contentBase64: 'UEsDBAo=',
        sha256: 'abc123',
        source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
    };

    await skill.previewImport(input);
    await skill.installImport({ input, conflictStrategy: 'replace' });

    assert.deepEqual(calls[0].args.input, {
        kind: 'archiveBase64',
        file_name: 'embedded-skill.ttskill',
        content_base64: 'UEsDBAo=',
        sha256: 'abc123',
        source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
    });
    assert.deepEqual(calls[1].args.request, {
        input: {
            kind: 'archiveBase64',
            file_name: 'embedded-skill.ttskill',
            content_base64: 'UEsDBAo=',
            sha256: 'abc123',
            source: { kind: 'preset', id: 'preset:openai:test', label: 'Test preset' },
        },
        conflictStrategy: 'replace',
    });
});

test('api.skill lists installed skill files by skill name', async () => {
    const { calls, skill } = await installHarness();

    await skill.listFiles({ name: 'test-skill' });

    assert.equal(calls[0].command, 'list_skill_files');
    assert.deepEqual(calls[0].args, { name: 'test-skill' });
});

test('api.skill deletes installed skills by skill name', async () => {
    const { calls, skill } = await installHarness();

    await skill.deleteSkill({ name: 'test-skill' });

    assert.equal(calls[0].command, 'delete_skill');
    assert.deepEqual(calls[0].args, { name: 'test-skill' });
});

test('api.skill picks import archives through the host dialog', async () => {
    const calls = [];
    globalThis.window = {
        __TAURITAVERN__: { api: {} },
    };

    const { installSkillApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/skill.js')));
    installSkillApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return '/tmp/test-skill.ttskill';
        },
    });

    const input = await globalThis.window.__TAURITAVERN__.api.skill.pickImportArchive();

    assert.deepEqual(input, { kind: 'archiveFile', path: '/tmp/test-skill.ttskill' });
    assert.equal(calls[0].command, 'plugin:dialog|open');
    assert.deepEqual(calls[0].args.options.filters, [
        { name: 'Agent Skill Archive', extensions: ['ttskill', 'zip'] },
    ]);
});

test('api.skill fails fast on unsupported import shapes', async () => {
    const { skill } = await installHarness();

    await assert.rejects(
        () => skill.previewImport({ kind: 'base64Zip', content: 'abc' }),
        /Unsupported skill import kind/,
    );
    await assert.rejects(
        () => skill.previewImport({ kind: 'inlineFiles', files: [] }),
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
        () => skill.deleteSkill({ name: '' }),
        /skill name is required/,
    );
});
