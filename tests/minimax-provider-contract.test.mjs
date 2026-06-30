import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const openaiSource = await readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8');
const secretsSource = await readFile(new URL('../src/scripts/secrets.js', import.meta.url), 'utf8');
const aiRoutesSource = await readFile(new URL('../src/tauri/main/routes/ai-routes.js', import.meta.url), 'utf8');
const rustSource = await readFile(new URL('../src-tauri/crates/tt-domain/src/models/chat_completion_source.rs', import.meta.url), 'utf8');
const rustConfigSource = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/config.rs', import.meta.url), 'utf8');
const rustPayloadSource = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/payload/minimax.rs', import.meta.url), 'utf8');
const rustRepositorySource = await readFile(new URL('../src-tauri/src/infrastructure/apis/http_chat_completion_repository/mod.rs', import.meta.url), 'utf8');
const rossAscendsSource = await readFile(new URL('../src/scripts/RossAscends-mods.js', import.meta.url), 'utf8');

test('MiniMax chat source is wired through frontend settings and backend source parsing', () => {
    assert.match(openaiSource, /MINIMAX:\s*'minimax'/);
    assert.match(openaiSource, /export const MINIMAX_ENDPOINT\s*=\s*\{/);
    assert.match(openaiSource, /minimax_model:\s*\['#model_minimax_select'/);
    assert.match(openaiSource, /minimax_endpoint:\s*\['#minimax_endpoint'/);
    assert.match(openaiSource, /case chat_completion_sources\.MINIMAX:\s*return settings\.minimax_model/);
    assert.match(aiRoutesSource, /source\.minimax_model/);

    assert.match(rustSource, /MiniMax/);
    assert.match(rustSource, /"minimax"[^=]+=> Some\(Self::MiniMax\)/s);
    assert.match(rustConfigSource, /MINIMAX_API_BASE/);
    assert.match(rustConfigSource, /MINIMAX_API_BASE_CN/);
    assert.match(rustConfigSource, /ChatCompletionSource::MiniMax => minimax_base_url/);
});

test('MiniMax generation keeps provider-specific request shaping in backend payload builder', () => {
    assert.match(openaiSource, /generate_data\.minimax_endpoint = settings\.minimax_endpoint \|\| MINIMAX_ENDPOINT\.GLOBAL/);
    assert.match(openaiSource, /clamp\(generate_data\.temperature, Number\.EPSILON, 1\.0\)/);
    assert.match(rustPayloadSource, /PromptProcessingType::MergeTools/);
    assert.match(rustPayloadSource, /M2_HER_MAX_TOKENS:\s*u64\s*=\s*2048/);
    assert.match(rustPayloadSource, /MINIMAX_ENDPOINT_PATH:\s*&str\s*=\s*"\/chat\/completions"/);
    assert.match(rustPayloadSource, /MINIMAX_REQUEST_FIELDS:\s*&\s*\[&str\]/);
    assert.doesNotMatch(rustPayloadSource, /openai::build/);
});

test('MiniMax secrets use generic provider key and expose settings visibility probe without PKCE changes', () => {
    assert.match(secretsSource, /\[SECRET_KEYS\.MINIMAX\]: 'MiniMax'/);
    assert.match(secretsSource, /\[SECRET_KEYS\.MINIMAX\]: '#api_key_minimax'/);
    assert.match(secretsSource, /export async function canViewSecrets\(\)/);
    assert.doesNotMatch(secretsSource, /code_challenge_method/);
});

test('MiniMax model-list bypass remains owned by application service', () => {
    assert.match(rustRepositorySource, /ChatCompletionSource::MiniMax => Err\(DomainError::InvalidData/);
    assert.doesNotMatch(rustRepositorySource, /ChatCompletionSource::MiniMax => Ok\(json!\(\{\s*"bypass"/s);
});

test('MiniMax participates in OpenAI-family startup autoconnect', () => {
    assert.match(
        rossAscendsSource,
        /secret_state\[SECRET_KEYS\.MINIMAX\]\s*&&\s*oai_settings\.chat_completion_source\s*==\s*chat_completion_sources\.MINIMAX/,
    );
});
