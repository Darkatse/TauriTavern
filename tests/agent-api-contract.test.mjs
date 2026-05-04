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

    const { installAgentApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent.js')));
    installAgentApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        },
    });

    return {
        calls,
        agent: globalThis.window.__TAURITAVERN__.api.agent,
    };
}

test('api.agent.profiles forwards profile commands with camelCase DTOs', async () => {
    const { calls, agent } = await installHarness();
    const profile = {
        schemaVersion: 1,
        kind: 'tauritavern.agentProfile',
        id: 'writer',
    };

    assert.ok(agent.profiles);
    await agent.profiles.list();
    await agent.profiles.load({ profileId: 'writer' });
    await agent.profiles.save({ profile });
    await agent.profiles.delete('writer');

    assert.deepEqual(calls, [
        { command: 'list_agent_profiles', args: undefined },
        { command: 'load_agent_profile', args: { dto: { profileId: 'writer' } } },
        { command: 'save_agent_profile', args: { dto: { profile } } },
        { command: 'delete_agent_profile', args: { dto: { profileId: 'writer' } } },
    ]);
});

test('api.agent.profiles fails fast on invalid profile inputs', async () => {
    const { agent } = await installHarness();

    await assert.rejects(
        () => agent.profiles.load({ profileId: '' }),
        /profileId is required/,
    );
    await assert.rejects(
        () => agent.profiles.delete(''),
        /profileId is required/,
    );
    await assert.rejects(
        () => agent.profiles.save(null),
        /profile must be an object/,
    );
});
