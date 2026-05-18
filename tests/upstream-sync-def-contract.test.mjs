import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('OpenAI tool reasoning sync preserves Tauri native reasoning lanes', async () => {
    const source = await readProjectFile('src/scripts/openai.js');

    assert.match(source, /export const tool_reasoning_modes = \{\s*DISABLED: 'disabled',\s*SINCE_LAST_USER: 'since_last_user',\s*ACTIVE_CHAIN: 'active_chain',\s*\}/);
    assert.match(source, /const interleaved_reasoning_providers = \[\s*chat_completion_sources\.OPENROUTER,\s*chat_completion_sources\.CUSTOM,\s*\]/);
    assert.match(source, /tool_reasoning_mode:\s*\['#tool_reasoning_mode', 'tool_reasoning_mode', false, false\]/);
    assert.match(source, /tool_call_recurse_limit:\s*\['#tool_call_recurse_limit', 'tool_call_recurse_limit', false, false\]/);
    assert.match(source, /tool_reasoning_mode:\s*tool_reasoning_modes\.DISABLED/);
    assert.match(source, /tool_call_recurse_limit:\s*5/);
    assert.match(source, /const canReplayProviderTurnMetadata = isSameModel && !isOtherGroupMember/);
    assert.match(source, /const reasoning = canReplayProviderTurnMetadata \? String\(chat\[j\]\?\.extra\?\.reasoning \?\? ''\) : ''/);
    assert.match(source, /const native = includeNative && canReplayProviderTurnMetadata \? chat\[j\]\?\.extra\?\.native : null/);
    assert.match(source, /&& canReplayProviderTurnMetadata/);
    assert.match(source, /if \(!canReplayProviderTurnMetadata && \(invocation\.signature \|\| invocation\.reasoning\)\) \{/);
    assert.match(source, /delete cloneInvocation\.reasoning/);
    assert.match(source, /toolCallMessage\.setToolCalls\(invocations, includeSignature, includeToolReasoning\)/);
    assert.match(source, /\.\.\.\(item\.reasoning \? \{ reasoning: item\.reasoning \} : \{\}\)/);
    assert.match(source, /\.\.\.\(item\.reasoningContent \? \{ reasoning_content: item\.reasoningContent \} : \{\}\)/);
    assert.match(source, /function getEffectiveToolReasoningMode\(settings = oai_settings\)/);
    assert.match(source, /ToolManager\.RECURSE_LIMIT = oai_settings\.tool_call_recurse_limit/);
});

test('ToolManager stores plaintext reasoning and failed tool invocations without dropping native metadata persistence', async () => {
    const source = await readProjectFile('src/scripts/tool-calling.js');

    assert.match(source, /@property \{string\?\} reasoning - The plaintext reasoning associated with this tool call turn\./);
    assert.match(source, /@property \{boolean\} \[error\] - Whether the tool invocation failed\./);
    assert.match(source, /return error;/);
    assert.match(source, /static async invokeFunctionTools\(data, \{ reasoningText = null \} = \{\}\)/);
    assert.match(source, /error:\s*true,\s*signature:\s*toolCall\.signature \|\| null,\s*reasoning:\s*reasoningText \|\| null/s);
    assert.match(source, /error:\s*false,\s*signature:\s*toolCall\.signature \|\| null,\s*reasoning:\s*reasoningText \|\| null/s);
    assert.match(source, /static async saveFunctionToolInvocations\(invocations, native = null, reasoningContent = null\)/);
    assert.match(source, /\.\.\.\(native !== null && native !== undefined \? \{ native \} : \{\}\)/);
    assert.match(source, /\.\.\.\(reasoningContent \? \{ tool_reasoning_content: reasoningContent \} : \{\}\)/);
});

test('Theme bgcol sync uses upstream ThemeGenerator flow with explicit overwrite semantics', async () => {
    const source = await readProjectFile('src/scripts/power-user.js');

    assert.match(source, /import \{ extractDominantColor, generateThemePalette, deriveBackgroundName \} from '\.\/util\/ThemeGenerator\.js';/);
    assert.match(source, /import \{ getBackgroundPath, isCustomBackgroundUrl \} from '\.\/backgrounds\.js';/);
    assert.match(source, /export function getThemeObject\(name\)/);
    assert.match(source, /async function setAvgBG\(args\)/);
    assert.match(source, /const force = isTrueBoolean\(args\?\.force\?\.toString\(\)\)/);
    assert.match(source, /const themeName = nameOverride \|\| `bgcol - \$\{bgName\}`/);
    assert.match(source, /themes\.some\(t => t\.name === themeName\) && !force/);
    assert.match(source, /const dominantRgb = extractDominantColor\(bgimg\)/);
    assert.match(source, /Object\.assign\(theme, palette\)/);
    assert.match(source, /name:\s*'bgcol'[\s\S]*name:\s*'force'[\s\S]*name:\s*'name'[\s\S]*name:\s*'bg'/);
});

test('World Info and Persona sync expose upstream-visible DEF behavior while keeping local descriptors', async () => {
    const [worldInfoSource, personasSource] = await Promise.all([
        readProjectFile('src/scripts/world-info.js'),
        readProjectFile('src/scripts/personas.js'),
    ]);

    assert.match(worldInfoSource, /const previousValue = \$\('#character_world'\)\.val\(\)/);
    assert.match(worldInfoSource, /if \(previousValue && !name\) \{/);
    assert.match(worldInfoSource, /data\.data\.character_book = undefined/);
    assert.match(worldInfoSource, /toastr\.info\(t`Embedded lorebook will be removed from this character\.`\)/);
    assert.match(worldInfoSource, /throw error/);

    assert.match(personasSource, /import \{ persona_description_positions, power_user \} from '\.\/power-user\.js';/);
    assert.match(personasSource, /export \{ persona_description_positions \};/);
    assert.match(personasSource, /addLongPressEvent\('#persona_lore_button'/);
    assert.match(personasSource, /case 'persona_lorebook_link':\s*await onPersonaLoreButtonClick\(\{ shiftKey: true, altKey: false \}\)/);
    assert.match(personasSource, /if \(selectedLorebook && !shiftKey && !altKey\) \{\s*openWorldInfoEditor\(selectedLorebook\)/);
    assert.match(personasSource, /escapeHtml\(temporary\.info\)\.replaceAll\('\\n', '<br \/>'\)/);
});
