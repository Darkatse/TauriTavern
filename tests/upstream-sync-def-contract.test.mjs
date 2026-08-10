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
    assert.match(source, /strip_old_tool_calls:\s*\['#strip_old_tool_calls', 'strip_old_tool_calls', true, false\]/);
    assert.match(source, /tool_reasoning_mode:\s*tool_reasoning_modes\.DISABLED/);
    assert.match(source, /tool_call_recurse_limit:\s*5/);
    assert.match(source, /strip_old_tool_calls:\s*false/);
    assert.match(source, /const projectedChat = projectToolTurns\(chat, stripOldToolCalls\)/);
    assert.doesNotMatch(source, /projection\.issues/);
    assert.match(source, /const canReplayProviderTurnMetadata = isSameModel && !isOtherGroupMember/);
    assert.match(source, /const reasoning = canReplayProviderTurnMetadata \? String\(contentMessage\?\.extra\?\.reasoning \?\? ''\) : ''/);
    assert.match(source, /const includeClaudeNative = usesClaudeMessagesSemantics\(oai_settings, currentModel\)/);
    assert.match(source, /&& \(!includeClaudeNative \|\| hasClaudeToolUse\(metadataMessage\?\.extra\?\.native\)\)/);
    assert.match(source, /&& canReplayProviderTurnMetadata/);
    assert.match(source, /if \(!canReplayProviderTurnMetadata && \(invocation\.signature \|\| invocation\.reasoning\)\) \{/);
    assert.match(source, /delete cloneInvocation\.reasoning/);
    assert.match(source, /chatMessage\.setToolCalls\(invocations, includeSignature, includeToolReasoning, assemblyTokenHandler\)/);
    assert.match(source, /const sourceCount = entry\.type === 'tool-turn' \? entry\.sourceIndices\.length : 1/);
    assert.match(source, /if \(Array\.isArray\(chatPrompt\.invocations\)\)/);
    assert.doesNotMatch(source, /canUseTools && Array\.isArray\(chatPrompt\.invocations\)/);
    assert.doesNotMatch(source, /canUseTools && Array\.isArray\(candidate\.invocations\)/);
    assert.match(source, /Message\.createAsync\('tool', invocation\.result, invocation\.id, assemblyTokenHandler\)/);
    assert.doesNotMatch(source, /invocation\.result \|\| '\[No content\]'/);
    assert.equal(source.match(/message\.content \|\| message\.tool_calls \|\| message\.role === 'tool'/g)?.length, 2);
    assert.equal(source.match(/item\.content \|\| item\.tool_calls \|\| item\.role === 'tool'/g)?.length, 1);
    assert.match(source, /sourceCount \+= chatPrompt\.sourceCount/);
    assert.match(source, /sourceCount \+= promptSource\.sourceCount \?\? 0/);
    assert.match(source, /openai_messages_count = chatSourceCount/);
    assert.match(source, /content: this\.content,[\s\S]*tool_calls: JSON\.stringify\(this\.tool_calls\)/);
    assert.doesNotMatch(source, /const toolCallMessage =/);
    assert.match(source, /\.\.\.\(item\.reasoning \? \{ reasoning: item\.reasoning \} : \{\}\)/);
    assert.match(source, /\.\.\.\(item\.reasoningContent \? \{ reasoning_content: item\.reasoningContent \} : \{\}\)/);
    assert.match(source, /function getEffectiveToolReasoningMode\(settings = oai_settings\)/);
    assert.match(source, /ToolManager\.RECURSE_LIMIT = oai_settings\.tool_call_recurse_limit/);
});

test('ToolManager stores plaintext reasoning and failed tool invocations without dropping native metadata persistence', async () => {
    const [source, scriptSource, openaiSource, styleSource] = await Promise.all([
        readProjectFile('src/scripts/tool-calling.js'),
        readProjectFile('src/script.js'),
        readProjectFile('src/scripts/openai.js'),
        readProjectFile('src/style.css'),
    ]);

    assert.match(source, /@property \{string\?\} reasoning - The plaintext reasoning associated with this tool call turn\./);
    assert.match(source, /@property \{boolean\} \[error\] - Whether the tool invocation failed\./);
    assert.match(source, /return error;/);
    assert.match(source, /static async invokeFunctionTools\(data, \{ reasoningText = null, onCallsReady = null, onInvocationComplete = null, toolResolver = null, signal = null \} = \{\}\)/);
    assert.match(source, /error:\s*true,\s*signature:\s*toolCall\.signature \|\| null,\s*reasoning:\s*reasoningText \|\| null/s);
    assert.match(source, /error:\s*false,\s*signature:\s*toolCall\.signature \|\| null,\s*reasoning:\s*reasoningText \|\| null/s);
    assert.match(source, /static async saveFunctionToolTurn\(invocations, ownerMessage, reasoningContent = null\)/);
    assert.match(source, /Tool .* returned a result that is not JSON-serializable/);
    assert.match(source, /Provider returned duplicate tool call id/);
    assert.match(source, /if \(result\.errors\.length > 0\) \{\s*return result;/);
    const invokeFunctionTools = source.slice(
        source.indexOf('static async invokeFunctionTools'),
        source.indexOf('\n    /**', source.indexOf('static async invokeFunctionTools') + 1),
    );
    assert.ok(invokeFunctionTools.indexOf('onCallsReady(') < invokeFunctionTools.indexOf('ToolManager.invokeFunctionTool('));
    assert.match(invokeFunctionTools, /projectProgress && onInvocationComplete\?\.\(invocation\)/);
    assert.match(source, /invocation\.result === undefined \? \['Status', 'Running…'\] : \['Result', invocation\.result\]/);
    assert.match(source, /ownerMessage\.tool_calls = toolCalls/);
    assert.match(source, /chat\.push\(\.\.\.toolMessages\)/);
    assert.match(source, /ownerMessage\.extra\.tool_reasoning_content = reasoningContent/);
    assert.doesNotMatch(source, /tool_call_standalone|chat\.splice\(insertionIndex|tool_invocations:\s*invocations/);
    assert.equal(scriptSource.match(/const shouldStopGeneration = !invocationResult\.invocations\.length \|\| invocationResult\.stealthCalls\.length/g)?.length, 2);
    assert.equal(scriptSource.match(/allowToolCalls: canAdvertiseToolCalls/g)?.length, 3);
    assert.equal(scriptSource.match(/ToolManager\.saveFunctionToolTurn\(invocationResult\.invocations, toolTurnOwner,/g)?.length, 2);
    assert.match(scriptSource, /const pendingToolInvocations = new WeakMap\(\)/);
    assert.match(scriptSource, /function getToolMessageHTML\(message, messageId\) \{[\s\S]*for \(let index = messageId - 1; index >= 0; index--\)[\s\S]*Array\.isArray\(calls\) && calls\.find\(call => call\.id === message\.tool_call_id\)[\s\S]*ToolManager\.formatToolInvocationMessage\(\[\{[\s\S]*result: message\.extra\?\.display_text \?\? message\.mes/);
    assert.equal(scriptSource.match(/getToolMessageHTML\(/g)?.length, 5);
    assert.match(scriptSource, /toggleClass\('smallSysMes', mes\?\.extra\?\.isSmallSys === true \|\| mes\.role === 'tool'\)/);
    assert.match(styleSource, /\.mes\.smallSysMes\[data-message-role="tool"\]:not\(:has\(\.edit_textarea\)\) \.ch_name \{[^}]*display: flex;[^}]*position: absolute;[^}]*z-index: 1;/s);
    assert.match(styleSource, /\.mes\.smallSysMes\[data-message-role="tool"\]:not\(:has\(\.edit_textarea\)\) \.mes_buttons > :not\(\.mes_edit\) \{\s*display: none;/);
    assert.match(styleSource, /\.mes\.smallSysMes\[data-message-role="tool"\] \.mes_text > details > summary \{\s*padding-inline-end: 2em;/);
    assert.match(scriptSource, /const invocations = pendingToolInvocations\.get\(chat\[messageId\]\) \?\? \[\]/);
    assert.match(scriptSource, /updateToolCallUI\(\$\(element\), ownerId\);[\s\S]*catch \(error\) \{[\s\S]*Failed to refresh pending tool calls/);
    assert.match(scriptSource, /pendingToolInvocations\.get\(ownerMessage\)\?\.find\([\s\S]*if \(!call\) \{\s*console\.error\([\s\S]*return;/);
    assert.match(scriptSource, /updateReasoningUI\(messageElement\);\s*updateToolCallUI\(messageElement, messageId\);/);
    assert.doesNotMatch(scriptSource, /syncMountedToolProjection|projectToolTurns/);
    assert.equal(scriptSource.match(/onCallsReady: calls => showPendingToolCalls\(toolTurnOwner, calls\)/g)?.length, 2);
    assert.equal(scriptSource.match(/onInvocationComplete: invocation => completePendingToolCall\(toolTurnOwner, invocation\)/g)?.length, 2);
    assert.equal(scriptSource.match(/finally \{\s*clearPendingToolCalls\(toolTurnOwner\)/g)?.length, 2);
    assert.doesNotMatch(scriptSource, /hasToolCalls && shouldDeleteMessage && await deleteLastMessage\(\)/);
    assert.match(scriptSource, /export async function deleteLastMessage\(\) \{\s*await deleteMessage\(chat\.length - 1\);\s*\}/);
    assert.match(scriptSource, /lastMessage\?\.role === 'tool' && \['append', 'continue', 'appendFinal', 'swipe'\]\.includes\(type\)\) \{\s*type = 'normal';\s*\}/);
    assert.match(scriptSource, /message\?\.role === 'tool' \|\| Array\.isArray\(message\?\.tool_calls\)/);
    assert.doesNotMatch(scriptSource, /getLastNonToolMessageId|toolCallProjection(?:Owner|Source|Tail)|getOwnedToolResultMessageIds/);
    assert.match(source, /static canPerformMultiSwipe\(type, settings = null\)/);
    assert.match(source, /static canAdvertiseToolCalls\(type, settings = null, model = null\)/);
    assert.match(source, /#modelDisablesToolCalls\(settings, model\)[\s\S]*gpt-5-chat-latest/);
    assert.match(openaiSource, /const canMultiSwipe = !agentMode && ToolManager\.canPerformMultiSwipe\(type, settings\)/);
    assert.equal(openaiSource.match(/!agentMode && allowToolCalls && ToolManager\.canAdvertiseToolCalls\(type, settings(?:, model)?\)/g)?.length, 2);
    assert.doesNotMatch(openaiSource, /delete generate_data\.(?:tools|tool_choice)/);
    assert.match(openaiSource, /return \[chatSourceCount, toolData\]/);
    assert.match(openaiSource, /return \[chat, activePromptManager\.tokenHandler\.counts, toolData\]/);
    assert.match(openaiSource, /chatCompletion\.setTokenBudget\([^;]+;\s*let chatSourceCount = 0;\s*let toolData = null;/);
    assert.match(openaiSource, /if \(toolData === undefined\) \{\s*await ToolManager\.registerFunctionToolsOpenAI\(generate_data\);\s*\} else if \(toolData\) \{\s*Object\.assign\(generate_data, toolData\);/);
    assert.equal(scriptSource.match(/allowToolCalls: canAdvertiseToolCalls, toolData/g)?.length, 2);
    assert.match(openaiSource, /clone\.reasoning = String\(chatPrompt\.reasoning \|\| previousAssistantReasoning \|\| ''\)/);
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
