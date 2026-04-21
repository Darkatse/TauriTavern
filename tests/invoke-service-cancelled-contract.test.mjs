import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('InvokeService surfaces CommandError::Cancelled as a plain message', async () => {
    const { createInvokeService } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/services/invokes/invoke-service.js'),
    );

    const invoke = async () => {
        throw { Cancelled: 'Generation cancelled by user' };
    };

    const service = createInvokeService({
        invoke,
        policies: {},
    });

    await assert.rejects(
        service.safeInvoke('get_chat_completions_generate', {}),
        (error) => {
            assert.ok(error instanceof Error);
            assert.equal(error.message, 'Generation cancelled by user');
            return true;
        },
    );
});

