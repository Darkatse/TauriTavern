import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { test } from 'node:test';

const REPO_ROOT = path.resolve(new URL('..', import.meta.url).pathname);
const sdSourcePath = path.join(REPO_ROOT, 'src', 'scripts', 'extensions', 'stable-diffusion', 'index.js');

const sdSource = await readFile(sdSourcePath, 'utf8');

test('Stable Diffusion SDCPP generation forwards the selected model', () => {
    assert.match(
        sdSource,
        /async function generateSdcppImage[\s\S]*?const payload = \{[\s\S]*?model:\s*extension_settings\.sd\.model\s*\|\|\s*undefined,/,
    );
});

test('Stable Diffusion /imagine gallery flag preserves upstream save semantics', () => {
    assert.match(sdSource, /if \(isFalseBoolean\(args\?\.gallery\)\) \{\s*characterName = '';\s*\}/);
    assert.match(
        sdSource,
        /const filename = characterName \? `\$\{characterName\}_\$\{humanizedDateTime\(\)\}` : humanizedDateTime\(\);/,
    );
    assert.match(sdSource, /new SlashCommandNamedArgument\(\s*'gallery',/);
    assert.match(sdSource, /gallery=false/);
});

test('Stable Diffusion media swipe keeps prompt refinement and dimensions in one flow', () => {
    assert.match(
        sdSource,
        /const refineArgs = \{\s*negative: savedNegative,\s*resolution: mediaAttachment\.width && mediaAttachment\.height \? `\$\{mediaAttachment\.width\}x\$\{mediaAttachment\.height\}` : null,\s*\};/,
    );
    assert.match(sdSource, /const prompt = await refinePrompt\(savedPrompt, refineArgs\);/);
    assert.match(
        sdSource,
        /dimensions = setTypeSpecificDimensions\(generationType, refineArgs\.resolution \? mediaAttachment : null\);/,
    );
    assert.match(sdSource, /result\.url = await sendGenerationRequest\(generationType, prompt, refineArgs\.negative,/);
    assert.match(
        sdSource,
        /if \(refineArgs\.resolution\) \{\s*result\.width = mediaAttachment\.width;\s*result\.height = mediaAttachment\.height;\s*\}/,
    );
});

test('Stable Diffusion backend request uses explicit credentials instead of provider-specific loose fields', async () => {
    const repositorySource = await readFile(
        path.join(REPO_ROOT, 'src-tauri', 'src', 'domain', 'repositories', 'stable_diffusion_repository.rs'),
        'utf8',
    );
    const serviceSource = await readFile(
        path.join(REPO_ROOT, 'src-tauri', 'src', 'application', 'services', 'stable_diffusion_service.rs'),
        'utf8',
    );

    assert.match(repositorySource, /pub enum SdRouteCredentials \{/);
    assert.doesNotMatch(repositorySource, /workers_ai_api_key/);
    assert.match(serviceSource, /SdRouteCredentials::WorkersAi \{ api_key \}/);
});

test('Workers AI image generation requires a selected model before dispatch', () => {
    assert.match(sdSource, /function hasSelectedModelOption\(\)\s*\{/);
    assert.match(sdSource, /for \(const option of modelSelect\.options\) \{\s*if \(option\.value === selectedModel\) \{/);
    assert.match(
        sdSource,
        /case sources\.workersai:\s*return !!oai_settings\.workers_ai_account_id && !!secret_state\[SECRET_KEYS\.WORKERS_AI\] && hasSelectedModelOption\(\);/,
    );
});
