import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const openaiSource = await readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8');
const secretsSource = await readFile(new URL('../src/scripts/secrets.js', import.meta.url), 'utf8');
const aiRoutesSource = await readFile(new URL('../src/tauri/main/routes/ai-routes.js', import.meta.url), 'utf8');
const indexHtmlSource = await readFile(new URL('../src/index.html', import.meta.url), 'utf8');
const rustSource = await readFile(new URL('../src-tauri/src/domain/repositories/chat_completion_repository.rs', import.meta.url), 'utf8');
const rustConfigSource = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/config.rs', import.meta.url), 'utf8');
const rustServiceSource = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/mod.rs', import.meta.url), 'utf8');
const rustPayloadSource = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/payload/claude_aws.rs', import.meta.url), 'utf8');
const rustPayloadMod = await readFile(new URL('../src-tauri/src/application/services/chat_completion_service/payload/mod.rs', import.meta.url), 'utf8');
const rustRepositorySource = await readFile(new URL('../src-tauri/src/infrastructure/apis/http_chat_completion_repository/mod.rs', import.meta.url), 'utf8');
const rustHttpClaudeAws = await readFile(new URL('../src-tauri/src/infrastructure/apis/http_chat_completion_repository/claude_aws.rs', import.meta.url), 'utf8');
const rustSecretSource = await readFile(new URL('../src-tauri/src/domain/models/secret.rs', import.meta.url), 'utf8');
const rossAscendsSource = await readFile(new URL('../src/scripts/RossAscends-mods.js', import.meta.url), 'utf8');

test('Claude on AWS Bedrock chat source is wired through frontend settings and backend source parsing', () => {
    assert.match(openaiSource, /CLAUDE_AWS:\s*'claude_aws'/);
    assert.match(openaiSource, /export const CLAUDE_AWS_REGION_DEFAULT\s*=\s*'us-east-1'/);
    assert.match(openaiSource, /claude_aws_model:\s*\['#model_claude_aws_select'/);
    assert.match(openaiSource, /claude_aws_region:\s*\['#claude_aws_region'/);
    assert.match(openaiSource, /case chat_completion_sources\.CLAUDE_AWS:\s*return settings\.claude_aws_model/);
    assert.match(aiRoutesSource, /source\.claude_aws_model/);
    assert.match(aiRoutesSource, /claude_aws_region:\s*String\(payload\.claude_aws_region/);

    assert.match(rustSource, /ClaudeAws/);
    assert.match(rustSource, /"claude_aws"[^=]+=> Some\(Self::ClaudeAws\)/s);
    assert.match(rustConfigSource, /CLAUDE_AWS_DEFAULT_REGION/);
    assert.match(rustConfigSource, /ChatCompletionSource::ClaudeAws => claude_aws_base_url/);
    assert.match(rustConfigSource, /https:\/\/bedrock-runtime\.\{region\}\.amazonaws\.com/);
});

test('Bedrock generation moves model into the URL path and pins anthropic_version', () => {
    assert.match(openaiSource, /generate_data\.claude_aws_region = \(settings\.claude_aws_region \|\| CLAUDE_AWS_REGION_DEFAULT\)\.trim\(\)/);
    assert.match(rustPayloadSource, /bedrock-2023-05-31/);
    assert.match(rustPayloadSource, /\/model\/\{model_id\}\/\{BEDROCK_INVOKE_SUFFIX\}/);
    assert.match(rustPayloadSource, /request_object\.remove\("stream"\)/);
    assert.match(rustPayloadSource, /request_object\.remove\("model"\)/);
    assert.match(rustPayloadMod, /ChatCompletionSource::ClaudeAws => Ok\(claude_aws::build\(payload\)\?\)/);
});

test('Bedrock HTTP repository uses Bearer auth and EventStream Accept for streaming', () => {
    assert.match(rustHttpClaudeAws, /apply_bearer_auth/);
    assert.match(rustHttpClaudeAws, /application\/vnd\.amazon\.eventstream/);
    assert.match(rustHttpClaudeAws, /\/invoke-with-response-stream/);
    assert.match(rustHttpClaudeAws, /decode_eventstream_payload/);
    assert.match(rustHttpClaudeAws, /BASE64_STANDARD\.decode/);
    assert.match(rustRepositorySource, /ChatCompletionSource::ClaudeAws => \{\s*claude_aws::generate/s);
    assert.match(rustRepositorySource, /ChatCompletionSource::ClaudeAws => \{\s*claude_aws::generate_stream/s);
});

test('Bedrock status check goes through list_models (no bypass), so Connect actually validates the key + region', () => {
    // application service no longer hard-bypasses ClaudeAws; the matches!() in
    // get_status must only list bypass-only sources (VertexAi, MiniMax).
    assert.doesNotMatch(
        rustServiceSource,
        /ChatCompletionSource::VertexAi[\s|]+ChatCompletionSource::MiniMax[\s|]+ChatCompletionSource::ClaudeAws/,
    );
    // repository now dispatches ClaudeAws list_models to the Bedrock module.
    assert.match(
        rustRepositorySource,
        /ChatCompletionSource::ClaudeAws => claude_aws::list_models/,
    );
});

test('Bedrock list_models calls the regional control plane and merges foundation-models + inference-profiles', () => {
    // Runtime base (bedrock-runtime.{region}...) must be rewritten to the
    // control-plane host (bedrock.{region}...).
    assert.match(rustHttpClaudeAws, /derive_control_plane_base/);
    assert.match(rustHttpClaudeAws, /bedrock-runtime\./);
    assert.match(rustHttpClaudeAws, /\/foundation-models\?byProvider=\{BEDROCK_ANTHROPIC_PROVIDER\}&byOutputModality=TEXT/);
    assert.match(rustHttpClaudeAws, /\/inference-profiles/);
    // Each merged entry tags its origin so the frontend can group by source.
    assert.match(rustHttpClaudeAws, /"source":\s*"foundation-model"/);
    assert.match(rustHttpClaudeAws, /"source":\s*"inference-profile"/);
});

test('Bedrock payload normalizes model IDs so Claude 4.6 / 4.7 contract resolution works', () => {
    // The Bedrock invoker keeps the original ID for the URL path...
    assert.match(rustPayloadSource, /endpoint_path\s*=\s*format!\("\/model\/\{model_id\}\/\{BEDROCK_INVOKE_SUFFIX\}"/);
    // ...but injects a normalized "claude-..." ID into the body so the Claude
    // builder's `starts_with("claude-opus-4-7")` etc. matches.
    assert.match(rustPayloadSource, /fn normalize_bedrock_model_id/);
    assert.match(rustPayloadSource, /"us\."/);
    assert.match(rustPayloadSource, /"global\."/);
    assert.match(rustPayloadSource, /"anthropic\."/);
    assert.match(rustPayloadSource, /"-v1"/);
});

test('Frontend dynamic model list path is wired for CLAUDE_AWS', () => {
    // noValidateSources no longer contains CLAUDE_AWS.
    const noValidateBlock = openaiSource.match(/const noValidateSources = \[([\s\S]*?)\];/);
    assert.ok(noValidateBlock, 'noValidateSources block must exist');
    assert.doesNotMatch(noValidateBlock[1], /CLAUDE_AWS/);
    // saveModelList rebuilds the Bedrock select from the live catalog.
    // We assert on two anchors that must co-locate inside the CLAUDE_AWS
    // branch of saveModelList: the select lookup and the subsequent empty()
    // call, without pinning a particular jQuery-chaining style.
    assert.match(
        openaiSource,
        /chat_completion_source === chat_completion_sources\.CLAUDE_AWS\)[\s\S]*?\$\(['"]#model_claude_aws_select['"]\)[\s\S]*?\.empty\(\)/,
    );
    // Bedrock entries are grouped by source tag (foundation-model vs profile).
    assert.match(openaiSource, /source === 'foundation-model'/);
    assert.match(openaiSource, /source === 'inference-profile'/);
});

test('Bedrock UI ships Claude 4.5 / 4.6 / 4.7 fallback options', () => {
    // Claude 4.x foundation models are INFERENCE_PROFILE-only — invoking the
    // raw `anthropic.claude-opus-4-*` id is rejected by Bedrock with
    // "on-demand throughput isn't supported", so we surface only the
    // cross-region inference-profile variants for 4.x.
    assert.match(indexHtmlSource, /value="us\.anthropic\.claude-opus-4-7"/);
    assert.match(indexHtmlSource, /value="us\.anthropic\.claude-opus-4-6-v1"/);
    assert.match(indexHtmlSource, /value="us\.anthropic\.claude-opus-4-5-20251101-v1:0"/);
    assert.match(indexHtmlSource, /value="us\.anthropic\.claude-sonnet-4-6"/);
    assert.match(indexHtmlSource, /value="us\.anthropic\.claude-haiku-4-5-20251001-v1:0"/);
    assert.match(indexHtmlSource, /value="global\.anthropic\.claude-opus-4-7"/);
    assert.match(indexHtmlSource, /value="global\.anthropic\.claude-opus-4-6-v1"/);
    // The raw 4.x foundation-model ids must NOT appear in the fallback
    // dropdown (they'd 400 with INVALID_PAYMENT_INSTRUMENT-style errors).
    assert.doesNotMatch(indexHtmlSource, /<option value="anthropic\.claude-opus-4-7"/);
    assert.doesNotMatch(indexHtmlSource, /<option value="anthropic\.claude-opus-4-6-v1"/);
    // Claude 3.x ON_DEMAND foundation models are still allowed.
    assert.match(indexHtmlSource, /value="anthropic\.claude-3-haiku-20240307-v1:0"/);
});

test('Bedrock secrets and connect flow expose the new bearer-token key', () => {
    assert.match(rustSecretSource, /CLAUDE_AWS:\s*&'static str\s*=\s*"api_key_claude_aws"/);
    assert.match(secretsSource, /CLAUDE_AWS:\s*'api_key_claude_aws'/);
    assert.match(secretsSource, /\[SECRET_KEYS\.CLAUDE_AWS\]:\s*'Claude on AWS Bedrock'/);
    assert.match(secretsSource, /\[SECRET_KEYS\.CLAUDE_AWS\]:\s*'#api_key_claude_aws'/);
    assert.match(openaiSource, /SECRET_KEYS\.CLAUDE_AWS,\s*selector:\s*'#api_key_claude_aws'/);
});

test('Bedrock UI exposes a dedicated form with region input and Bedrock model select', () => {
    assert.match(indexHtmlSource, /<option value="claude_aws">Claude on AWS Bedrock<\/option>/);
    assert.match(indexHtmlSource, /id="claude_aws_form"\s+data-source="claude_aws"/);
    assert.match(indexHtmlSource, /id="api_key_claude_aws"/);
    assert.match(indexHtmlSource, /id="claude_aws_region"/);
    assert.match(indexHtmlSource, /id="model_claude_aws_select"/);
    assert.match(indexHtmlSource, /anthropic\.claude-sonnet-4-20250514-v1:0/);
});

test('Bedrock participates in OpenAI-family startup autoconnect', () => {
    assert.match(
        rossAscendsSource,
        /secret_state\[SECRET_KEYS\.CLAUDE_AWS\]\s*&&\s*oai_settings\.chat_completion_source\s*==\s*chat_completion_sources\.CLAUDE_AWS/,
    );
});

test('Bedrock exposes the reasoning UI controls (show_thoughts, reasoning_effort, thinking-budget hint)', () => {
    // The reasoning UI was previously hidden for the claude_aws source, which made
    // the globally-shared oai_settings.reasoning_effort silently leak into Bedrock
    // requests and force adaptive thinking on every turn. Each of these controls
    // must include `claude_aws` in its data-source whitelist.
    const reasoningEffortRow = indexHtmlSource.match(
        /<div[^>]*data-source="([^"]+)"[^>]*>\s*<div[^>]+title="Constrains effort on reasoning[^>]*"/,
    );
    assert.ok(reasoningEffortRow, 'Reasoning Effort wrapper div must exist');
    assert.ok(
        reasoningEffortRow[1].split(',').map(s => s.trim()).includes('claude_aws'),
        `Reasoning Effort wrapper missing claude_aws (got: ${reasoningEffortRow[1]})`,
    );

    const showThoughtsRow = indexHtmlSource.match(
        /<div[^>]*data-source="([^"]+)"[^>]*>\s*<label[^>]*for="openai_show_thoughts"/,
    );
    assert.ok(showThoughtsRow, 'Show Thoughts wrapper div must exist');
    assert.ok(
        showThoughtsRow[1].split(',').map(s => s.trim()).includes('claude_aws'),
        `Show Thoughts wrapper missing claude_aws (got: ${showThoughtsRow[1]})`,
    );

    const thinkingBudgetHint = indexHtmlSource.match(
        /<div[^>]*data-source="([^"]+)"[^>]*data-i18n="Allocates a portion of the response length for thinking[^"]*"/,
    );
    assert.ok(thinkingBudgetHint, 'Thinking-budget hint paragraph must exist');
    assert.ok(
        thinkingBudgetHint[1].split(',').map(s => s.trim()).includes('claude_aws'),
        `Thinking-budget hint missing claude_aws (got: ${thinkingBudgetHint[1]})`,
    );
});
