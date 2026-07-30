import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Generate wrapper uses an in-flight lifecycle gate around GenerateInternal()', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /const generationIdleGate = createGenerationIdleGate\(\);/);
    assert.match(source, /let generationInFlightCount = 0;/);
    assert.match(source, /let generationHistoryLocator = null;/);
    assert.match(source, /export function waitForGenerationIdle\(\)\s*{\s*return generationIdleGate\.wait\(\);\s*}/s);
    assert.match(source, /async function enterGeneration\(dryRun\)[\s\S]*generationInFlightCount = 1;[\s\S]*generationHistoryLocator = null;[\s\S]*if \(dryRun\) return;[\s\S]*getActiveChatSnapshot\(\)\.ref;[\s\S]*invoke\('chat_history_generation_started', \{ locator: generationHistoryLocator \}\);/);
    assert.match(source, /async function exitGeneration\(\)[\s\S]*const locator = generationHistoryLocator;[\s\S]*generationHistoryLocator = null;[\s\S]*invoke\('chat_history_generation_finished', \{ locator \}\);[\s\S]*finally \{\s*generationIdleGate\.markIdle\(\);/s);
    assert.match(source, /export async function Generate\(type, options = \{\}, dryRun = false\)\s*{\s*await enterGeneration\(dryRun\);\s*try {\s*return await GenerateInternal\(type, options, dryRun\);\s*} catch \(error\) {\s*cleanupGenerationAfterUnhandledError\(type, dryRun\);\s*throw error;\s*} finally {\s*await exitGeneration\(\);\s*}\s*}/s);
    assert.match(source, /async function GenerateInternal\(/);
});

test('Unhandled foreground Generate errors reuse the legacy unblock path', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /function cleanupGenerationAfterUnhandledError\(type, dryRun\)\s*{/);
    assert.match(source, /shouldUnblockGenerationAfterUnhandledError\(\{\s*dryRun,\s*isSendPress: is_send_press,\s*isBodyGenerating: document\.body\.dataset\.generating === 'true',\s*isGroupGenerating: is_group_generating,\s*}\)/s);
    assert.match(source, /cleanupGenerationAfterUnhandledError[\s\S]*unblockGeneration\(type\);/);
});

test('stable chat-history reasons cover provider, final, transport, and agent commits', async () => {
    const [script, groups, transport, commit, routes, agentBridge] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/transport.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/tauri/chat/commit.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/tauri/main/routes/chat-routes.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/tauri/main/api/agent-chat-commit-bridge.js'), 'utf8'),
    ]);

    assert.match(script, /sendMessageAsUserAtProviderBarrier\(textareaText, messageBias\)/);
    assert.match(script, /sendMessageAsUserAtProviderBarrier\(oai_settings\.send_if_empty\.trim\(\), messageBias\)/);
    assert.match(script, /saveChatConditional\(this\.type === 'impersonate'[\s\S]*CHAT_COMMIT_REASON\.MUTATION[\s\S]*CHAT_COMMIT_REASON\.GENERATION_CHECKPOINT\)/);
    assert.match(script, /saveChatConditional\(isImpersonate[\s\S]*CHAT_COMMIT_REASON\.MUTATION[\s\S]*CHAT_COMMIT_REASON\.GENERATION_CHECKPOINT\)/);
    assert.match(script, /saveChatUnsafe\(\{ chatName, withMetadata, mesId, force: true, chatData, commitReason \}\)/);
    assert.match(groups, /saveGroupChatUnsafe\(groupId, shouldSaveGroup, true, commitReason\)/);
    assert.match(groups, /sendMessageAsUser\(userInput, bias\.messageBias\)/);
    assert.match(script, /saveChatConditional\(CHAT_COMMIT_REASON\.MAINTENANCE\)/);
    assert.match(groups, /saveGroupChat\(groupId, false, false, CHAT_COMMIT_REASON\.MAINTENANCE\)/);
    assert.match(transport, /MAINTENANCE: 'maintenance'/);

    assert.match(script, /commit_reason:\s*commitReason/);
    assert.match(groups, /commit_reason:\s*commitReason/);
    assert.equal(transport.match(/commit_reason: commitReason/g)?.length ?? 0, 0);
    assert.match(commit, /const normalizedCommitReason = commitReason \?\? 'mutation';/);
    assert.match(commit, /invoke\('finish_chat_commit', \{\s*sessionId,\s*expectedSize: offset,\s*commitReason: normalizedCommitReason,/s);
    assert.equal(routes.match(/commitReason: body\?\.commit_reason/g)?.length, 2);
    assert.match(agentBridge, /handleChatCommitRequested[\s\S]*state\.persistChat\(script, CHAT_COMMIT_REASON\.GENERATION_CHECKPOINT\)/);
    assert.match(agentBridge, /handlePersistentStateMetadataUpdateRequested[\s\S]*state\.persistChat\(script, CHAT_COMMIT_REASON\.MUTATION\)/);
    assert.match(agentBridge, /saveGroupChat\([\s\S]*commitReason,[\s\S]*script\.saveChat\(\{ commitReason \}\)/);
});

test('Unhandled error cleanup only targets foreground UI lifecycle leaks', async () => {
    const { shouldUnblockGenerationAfterUnhandledError } = await import('../src/scripts/util/generation-lifecycle.js');

    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: true,
        isSendPress: true,
        isBodyGenerating: true,
        isGroupGenerating: false,
    }), false);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: true,
        isBodyGenerating: false,
        isGroupGenerating: true,
    }), false);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: true,
        isBodyGenerating: false,
        isGroupGenerating: false,
    }), true);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: false,
        isBodyGenerating: true,
        isGroupGenerating: false,
    }), true);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: false,
        isBodyGenerating: false,
        isGroupGenerating: false,
    }), false);
});
