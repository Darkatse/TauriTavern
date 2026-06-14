import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// Mock Tauri Environment
let lastInvokedCommand = null;
let lastInvokedArgs = null;

global.window = {
    __TAURI_RUNNING__: true,
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
const { updateTauriTavernSettings, openDialog, setDataRoot } = await import(pathToFileURL(tauriBridgePath).href);

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

    await t.test('rejects arrays', async () => {
        await assert.rejects(
            updateTauriTavernSettings([]),
            /Invalid TauriTavern settings DTO/
        );
    });

    await t.test('rejects primitives (string)', async () => {
        await assert.rejects(
            updateTauriTavernSettings('settings_string'),
            /Invalid TauriTavern settings DTO/
        );
    });

    await t.test('rejects primitives (number)', async () => {
        await assert.rejects(
            updateTauriTavernSettings(42),
            /Invalid TauriTavern settings DTO/
        );
    });

    await t.test('rejects primitives (boolean)', async () => {
        await assert.rejects(
            updateTauriTavernSettings(true),
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

test('openDialog contract validation', async (t) => {
    await t.test('accepts valid plain object options', async () => {
        lastInvokedCommand = null;
        lastInvokedArgs = null;
        const options = { title: 'Choose File' };
        await openDialog(options);
        assert.equal(lastInvokedCommand, 'plugin:dialog|open');
        assert.deepEqual(lastInvokedArgs.options, options);
    });

    await t.test('normalizes omitted options to empty object', async () => {
        lastInvokedCommand = null;
        lastInvokedArgs = null;
        await openDialog();
        assert.equal(lastInvokedCommand, 'plugin:dialog|open');
        assert.deepEqual(lastInvokedArgs.options, {});
    });

    await t.test('normalizes undefined to empty object', async () => {
        lastInvokedCommand = null;
        lastInvokedArgs = null;
        await openDialog(undefined);
        assert.equal(lastInvokedCommand, 'plugin:dialog|open');
        assert.deepEqual(lastInvokedArgs.options, {});
    });

    await t.test('rejects null', async () => {
        await assert.rejects(
            openDialog(null),
            /Invalid dialog options: expected an object/
        );
    });

    await t.test('rejects arrays', async () => {
        await assert.rejects(
            openDialog([]),
            /Invalid dialog options: expected an object/
        );
    });

    await t.test('rejects primitives (string)', async () => {
        await assert.rejects(
            openDialog('some_option'),
            /Invalid dialog options: expected an object/
        );
    });

    await t.test('rejects primitives (number)', async () => {
        await assert.rejects(
            openDialog(123),
            /Invalid dialog options: expected an object/
        );
    });

    await t.test('rejects primitives (boolean)', async () => {
        await assert.rejects(
            openDialog(false),
            /Invalid dialog options: expected an object/
        );
    });
});

test('setDataRoot contract validation', async (t) => {
    await t.test('accepts valid non-empty path string', async () => {
        lastInvokedCommand = null;
        lastInvokedArgs = null;
        await setDataRoot('/valid/path');
        assert.equal(lastInvokedCommand, 'set_data_root');
        assert.equal(lastInvokedArgs.data_root, '/valid/path');
    });

    await t.test('rejects empty string', async () => {
        await assert.rejects(
            setDataRoot(''),
            /Invalid data root path/
        );
    });

    await t.test('rejects whitespace-only string', async () => {
        await assert.rejects(
            setDataRoot('   '),
            /Invalid data root path/
        );
    });

    await t.test('rejects non-string values', async () => {
        await assert.rejects(
            setDataRoot(42),
            /Invalid data root path/
        );
        await assert.rejects(
            setDataRoot(null),
            /Invalid data root path/
        );
    });
});
