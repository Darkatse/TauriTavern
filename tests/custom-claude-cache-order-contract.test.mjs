import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const serviceSource = await readFile(
    new URL('../src-tauri/src/application/services/chat_completion_service/mod.rs', import.meta.url),
    'utf8',
);

function assertBodyOverridesBeforePromptCaching(functionNeedle) {
    const functionStart = serviceSource.indexOf(functionNeedle);
    assert.ok(functionStart >= 0, `${functionNeedle} should exist`);

    const bodyOverrideIndex = serviceSource.indexOf(
        'additional_parameters.apply_body_overrides(&mut upstream_payload)?;',
        functionStart,
    );
    const promptCachingIndex = serviceSource.indexOf(
        'self.apply_tauritavern_prompt_caching(',
        functionStart,
    );

    assert.ok(bodyOverrideIndex >= 0, `${functionNeedle} should apply body overrides`);
    assert.ok(promptCachingIndex >= 0, `${functionNeedle} should apply prompt caching`);
    assert.ok(
        bodyOverrideIndex < promptCachingIndex,
        `${functionNeedle} must apply custom body overrides before prompt caching`,
    );
}

test('chat generation applies final body overrides before prompt caching', () => {
    assertBodyOverridesBeforePromptCaching('async fn execute_generate(');
});

test('stream generation applies final body overrides before prompt caching', () => {
    assertBodyOverridesBeforePromptCaching('pub async fn generate_stream(');
});
