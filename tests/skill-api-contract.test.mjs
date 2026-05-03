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
});
