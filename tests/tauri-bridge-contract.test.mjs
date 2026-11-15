import { installHostIdentity } from './helpers/host-identity.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

test.beforeEach(t => t.after(installHostIdentity()));

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// Mock Tauri Environment
let lastInvokedCommand = null;
let lastInvokedArgs = null;

global.window = {
    __TAURI__: {
        core: {
            invoke: async (command, args) => {
                lastInvokedCommand = command;
                lastInvokedArgs = args;
                return { success: true };
            }
        }
    }
};

// Import the bridge under test
const tauriBridgePath = path.join(REPO_ROOT, 'src/tauri-bridge.js');
const { updateTauriTavernSettings } = await import(pathToFileURL(tauriBridgePath).href);

test('updateTauriTavernSettings contract validation', async (t) => {
    await t.test('accepts valid plain object', async () => {
        lastInvokedCommand = null;
        lastInvokedArgs = null;
        const dto = { theme: 'dark' };
        await updateTauriTavernSettings(dto);
        assert.equal(lastInvokedCommand, 'update_tauritavern_settings');
        assert.deepEqual(lastInvokedArgs.dto, dto);
    });

    await t.test('rejects null', async () => {
        await assert.rejects(
            updateTauriTavernSettings(null),
            /Invalid TauriTavern settings DTO/
        );
    });

    await t.test('rejects class instances', async () => {
        class Settings {}
        await assert.rejects(
            updateTauriTavernSettings(new Settings()),
            /Invalid TauriTavern settings DTO/
        );
    });
});
