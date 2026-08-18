import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Connect authorizes the selected endpoint before saving secrets or checking status', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/openai.js'), 'utf8');
    const start = source.indexOf('async function onConnectButtonClick');
    const end = source.indexOf('\nfunction toggleChatCompletionForms', start);
    const handler = source.slice(start, end);

    const authorization = handler.indexOf('authorizeChatCompletionEndpoint');
    const secretWrite = handler.indexOf('await writeSecret');
    const statusCheck = handler.indexOf('await getStatusOpen');

    assert.ok(authorization >= 0);
    assert.ok(authorization < secretWrite);
    assert.ok(secretWrite < statusCheck);
    assert.match(handler, /const promptForLocalEndpoint = e\.originalEvent\?\.isTrusted === true/);
    assert.match(handler, /getCurrentLocale\(\), promptForLocalEndpoint/);
    assert.match(handler, /toastr\.info\(t`Local endpoint approval required\. Click Connect to continue\.`\)/);
    assert.match(handler, /if \(!allowed\)/);
});

test('native authorization serializes dialogs without publishing expected command errors', async () => {
    const source = await readFile(path.join(
        REPO_ROOT,
        'src-tauri/crates/tauritavern/src/presentation/commands/local_endpoint_access_commands.rs',
    ), 'utf8');

    assert.match(source, /DIALOG_GATE\.lock\(\)\.await/);
    assert.doesNotMatch(source, /try_lock|map_command_error/);
});
