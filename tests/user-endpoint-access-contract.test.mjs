import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function source(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('every chat completion request entry point uses the native authorization gate', async () => {
    const commands = await source(
        'src-tauri/crates/tauritavern/src/presentation/commands/chat_completion_commands.rs',
    );

    assert.equal((commands.match(/\.resolve_status_user_endpoint\(&dto\)\?/g) ?? []).length, 1);
    assert.equal((commands.match(/\.resolve_generate_user_endpoint\(&dto\)\?/g) ?? []).length, 2);
    assert.equal((commands.match(/\n    ensure_user_endpoint_access\(/g) ?? []).length, 3);
});

test('granted endpoints bypass the dialog gate while first-time authorization stays serialized', async () => {
    const access = await source(
        'src-tauri/crates/tauritavern/src/presentation/commands/user_endpoint_access.rs',
    );

    const gateIndex = access.indexOf('DIALOG_GATE.lock().await');
    const grantChecks = [...access.matchAll(/access_service\.is_granted\(&endpoint\)\.await/g)]
        .map(match => match.index);

    assert.equal(grantChecks.length, 2);
    assert.ok(grantChecks[0] < gateIndex && gateIndex < grantChecks[1]);
    assert.match(access, /access_service\.grant\(endpoint\)\.await/);
    assert.doesNotMatch(access, /addresses|resolve_host|is_private/);
});

test('compatibility routes supply locale and no frontend preflight remains', async () => {
    const routes = await source('src/tauri/main/routes/ai-routes.js');
    const openai = await source('src/scripts/openai.js');

    assert.equal((routes.match(/locale: getSillyTavernLocale\(\)/g) ?? []).length, 3);

    assert.doesNotMatch(openai, /authorizeChatCompletionEndpoint|promptForEndpoint|isTrusted/);
});
