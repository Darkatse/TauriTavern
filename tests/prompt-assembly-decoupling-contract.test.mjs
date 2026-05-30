import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('connectionRef prompt assembly overlays model binding instead of validating preset source', async () => {
    const source = await readProjectFile('src-tauri/src/application/services/prompt_assembly_service.rs');

    assert.match(source, /apply_model_binding_to_prompt_settings/);
    assert.match(source, /"deepseek"\s*=>\s*Ok\("deepseek_model"\)/);
    assert.doesNotMatch(source, /preset_source_required/);
    assert.doesNotMatch(source, /model_source_mismatch/);
});

test('requiresConfiguration prompt assembly fails fast before frontend broker handoff', async () => {
    const source = await readProjectFile('src-tauri/src/application/services/prompt_assembly_service.rs');
    const guardIndex = source.indexOf('ensure_profile_model_configured(&profile)?;');
    const brokerIndex = source.indexOf('AgentPromptAssemblyModeDto::FrontendPromptAssembly');

    assert.ok(guardIndex >= 0, 'missing requiresConfiguration prompt assembly guard');
    assert.ok(brokerIndex > guardIndex, 'model configuration guard must run before broker handoff');
});

test('frontend prompt assembly normalizes effective settings without resolving model from defaults', async () => {
    const [brokerSource, openaiSource] = await Promise.all([
        readProjectFile('src/tauri/main/api/agent-prompt-assembly.js'),
        readProjectFile('src/scripts/openai.js'),
    ]);

    assert.match(openaiSource, /export function normalizeChatCompletionSettingsForPromptAssembly/);
    assert.match(brokerSource, /request\.modelId\s*\|\|\s*openai\.getChatCompletionModel\(request\.settings\)/);
    assert.match(brokerSource, /openai\.normalizeChatCompletionSettingsForPromptAssembly\(request\.settings\)/);
});
