import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('chat completion custom models are implemented through the shared model-control contract', async () => {
    const openaiSource = await readProjectFile('src/scripts/openai.js');

    assert.match(openaiSource, /const chatCompletionModelControls = \{/);
    assert.match(openaiSource, /export function getChatCompletionModelControl/);
    assert.match(openaiSource, /\[chat_completion_sources\.MINIMAX\]:\s*\{\s*selector:\s*'#model_minimax_select',\s*settingKey:\s*'minimax_model'[\s\S]*?supportsCustomModels:\s*true\s*\}/);
    assert.match(openaiSource, /\[chat_completion_sources\.WORKERS_AI\]:\s*\{\s*selector:\s*'#model_workers_ai_select',\s*settingKey:\s*'workers_ai_model'[\s\S]*?supportsCustomModels:\s*true\s*\}/);
    assert.match(openaiSource, /custom_models_by_source:\s*\['',\s*'custom_models_by_source',\s*false,\s*true\]/);
    assert.match(openaiSource, /custom_models_by_source:\s*\{\}/);
    assert.match(openaiSource, /function normalizeKnownCustomModelsStore/);
    assert.match(openaiSource, /const control = getChatCompletionModelControl\(source\);\s*if \(!control\?\.supportsCustomModels\) \{\s*continue;\s*\}/);
    const customModelsStoreBody = openaiSource.match(/function getCustomModelsStore[\s\S]*?\n}\n\nfunction normalizeKnownCustomModelsStore/)?.[0] ?? '';
    assert.doesNotMatch(customModelsStoreBody, /normalizeCustomModelsForSource/);
    assert.match(openaiSource, /function applyCustomModelOptionsForSource/);
    assert.match(openaiSource, /function handleCustomModelSelectAction/);
    assert.match(openaiSource, /await handleCustomModelSelectAction\(this,\s*value\)/);
    assert.match(openaiSource, /appendOption\(actionGroup,\s*t`Manage custom models\.\.\.`,\s*manage_custom_chat_completion_models_option\)/);
    assert.match(openaiSource, /onClosing:\s*\(popup\) => \{/);
    assert.match(openaiSource, /return addDraftModel\(\{ selectExisting: true \}\)/);
    assert.match(openaiSource, /modelId === currentModel && isCustomModelValueForSource\(control\.source,\s*modelId\)/);
    assert.match(openaiSource, /Popup\.show\.text\(t`Cannot delete current custom model`,\s*t`Switch to another model before deleting it\.`\)/);
    assert.doesNotMatch(openaiSource, /custom_chat_completion_model_option/);
    assert.doesNotMatch(openaiSource, /t`Custom model\.\.\.`/);
});

test('dynamic model lists preserve explicit custom selections instead of falling back silently', async () => {
    const openaiSource = await readProjectFile('src/scripts/openai.js');

    assert.match(openaiSource, /function chooseModelOrCurrentCustom/);
    assert.match(openaiSource, /setModelSelectValue\(chat_completion_sources\.OPENAI,\s*model\)/);
    assert.match(openaiSource, /setModelSelectValue\(chat_completion_sources\.MISTRALAI,\s*oai_settings\.mistralai_model\)/);
    assert.match(openaiSource, /oai_settings\.workers_ai_model = chooseModelOrCurrentCustom\(\s*chat_completion_sources\.WORKERS_AI,/);
    assert.match(openaiSource, /setModelSelectValue\(chat_completion_sources\.WORKERS_AI,\s*oai_settings\.workers_ai_model\)/);
    assert.match(openaiSource, /setModelSelectValue\(chat_completion_sources\.COMETAPI,\s*oai_settings\.cometapi_model\)/);
    assert.match(openaiSource, /!hasModelsLoaded && !isCustomModelValueForSource\(chat_completion_sources\.OPENROUTER,\s*value\)/);
    assert.match(openaiSource, /!hasModelsLoaded && !isCustomModelValueForSource\(chat_completion_sources\.WORKERS_AI,\s*value\)/);
});

test('/model command uses the chat completion registry and excludes select action sentinels', async () => {
    const slashCommandsSource = await readProjectFile('src/scripts/slash-commands.js');

    assert.match(slashCommandsSource, /getChatCompletionModelControl/);
    assert.match(slashCommandsSource, /isCustomModelActionValue/);
    assert.match(slashCommandsSource, /addAndSelectCustomModelForSource/);
    assert.match(slashCommandsSource, /\.filter\(x => !isCustomModelActionValue\(x\.value\)\)/);
    assert.match(slashCommandsSource, /supportsCustomModels && customModelSource[\s\S]*addAndSelectCustomModelForSource\(customModelSource,\s*model\)/);
    assert.doesNotMatch(slashCommandsSource, /\{\s*id:\s*'model_openai_select',\s*api:\s*'openai'/);
});
