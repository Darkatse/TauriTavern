import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const ECOSYSTEM_EVENTS = {
    CHAT_RENAMED: 'chat_renamed',
    PERSONA_CHANGED: 'persona_changed',
    PERSONA_CREATED: 'persona_created',
    PERSONA_UPDATED: 'persona_updated',
    PERSONA_RENAMED: 'persona_renamed',
    PERSONA_DELETED: 'persona_deleted',
    TTS_JOB_STARTED: 'tts_job_started',
    TTS_AUDIO_READY: 'tts_audio_ready',
    TTS_JOB_COMPLETE: 'tts_job_complete',
    ITEMIZED_PROMPTS_LOADED: 'itemized_prompts_loaded',
    ITEMIZED_PROMPTS_SAVED: 'itemized_prompts_saved',
    ITEMIZED_PROMPTS_DELETED: 'itemized_prompts_deleted',
};

test('events expose SillyTavern 1.18 ecosystem additions with upstream values', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/events.js'), 'utf8');

    for (const [eventName, eventValue] of Object.entries(ECOSYSTEM_EVENTS)) {
        assert.match(source, new RegExp(`\\b${eventName}\\s*:\\s*'${eventValue}'`));
    }
});

test('1.18 ecosystem events are emitted by their runtime owners', async () => {
    const [script, personas, tts, itemizedPrompts, welcomeScreen] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/personas.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/extensions/tts/index.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/itemized-prompts.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/welcome-screen.js'), 'utf8'),
    ]);

    assert.match(script, /eventSource\.emit\(event_types\.CHAT_RENAMED,\s*eventData\)/);
    assert.match(welcomeScreen, /eventSource\.on\(event_types\.CHAT_RENAMED,\s*\(\{\s*avatarId,\s*groupId,\s*oldFileName,\s*newFileName\s*\}\)\s*=>\s*\{/);
    assert.match(welcomeScreen, /PinnedChatsManager\.rename\(\{\s*avatar:\s*avatarId,\s*group:\s*groupId,\s*file_name:\s*oldFileName\s*\},\s*newFileName\)/);

    for (const eventName of [
        'PERSONA_CHANGED',
        'PERSONA_CREATED',
        'PERSONA_UPDATED',
        'PERSONA_RENAMED',
        'PERSONA_DELETED',
    ]) {
        assert.match(personas, new RegExp(`eventSource\\.emit\\(event_types\\.${eventName}\\b`));
    }
    assert.match(personas, /PERSONA_CHANGED,\s*user_avatar/);
    assert.match(personas, /PERSONA_RENAMED,\s*\{\s*avatarId,\s*oldName:\s*currentName,\s*newName\s*\}/);
    assert.match(personas, /PERSONA_DELETED,\s*\{\s*avatarId,\s*name\s*\}/);

    assert.match(tts, /TTS_JOB_STARTED,\s*\{\s*messageId,\s*characterName:\s*char,\s*text,\s*voiceId\s*\}/);
    assert.match(tts, /TTS_AUDIO_READY,\s*\{/);
    assert.match(tts, /audio:\s*audioResult\.audioBlob/);
    assert.match(tts, /mimeType:\s*audioResult\.mimeType/);
    assert.match(tts, /TTS_JOB_COMPLETE,\s*\{\s*messageId,\s*characterName:\s*char\s*\}/);

    assert.match(itemizedPrompts, /ITEMIZED_PROMPTS_LOADED,\s*\{\s*chatId:\s*chatId\s*\}/);
    assert.match(itemizedPrompts, /ITEMIZED_PROMPTS_SAVED,\s*\{\s*chatId:\s*chatId\s*\}/);
    assert.match(itemizedPrompts, /ITEMIZED_PROMPTS_DELETED,\s*\{\s*chatId:\s*chatId,\s*all:\s*false\s*\}/);
    assert.match(itemizedPrompts, /ITEMIZED_PROMPTS_DELETED,\s*\{\s*all:\s*true\s*\}/);
});

test('TauriTavern keeps SETTINGS_LOADED in auto-fire events', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/events.js'), 'utf8');

    assert.match(
        source,
        /new\s+EventEmitter\(\[\s*event_types\.APP_READY,\s*event_types\.APP_INITIALIZED,\s*event_types\.SETTINGS_LOADED\s*\]\)/,
    );
});

test('chat rename events use backend-committed normalized file names', async () => {
    const [script, welcomeScreen, route, chatCommand, groupCommand, chatService, groupService] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/welcome-screen.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/tauri/main/routes/chat-routes.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src-tauri/src/presentation/commands/chat_commands.rs'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src-tauri/src/presentation/commands/group_chat_commands.rs'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src-tauri/src/application/services/chat_service.rs'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src-tauri/src/application/services/group_chat_service.rs'), 'utf8'),
    ]);

    assert.match(route, /const sanitizedFileName = await context\.safeInvoke\('rename_group_chat'/);
    assert.match(route, /const sanitizedFileName = await context\.safeInvoke\('rename_chat'/);
    assert.match(route, /return jsonResponse\(\{\s*ok:\s*true,\s*sanitizedFileName\s*\}\)/);
    assert.match(script, /const committedFileName = data\.sanitizedFileName;/);
    assert.match(script, /newFileName:\s*`\$\{committedFileName\}\.jsonl`/);
    assert.match(script, /return committedFileName;/);
    assert.doesNotMatch(script, /newFileName:\s*body\.renamed_file/);
    assert.match(welcomeScreen, /const committedFileName = await renameGroupOrCharacterChat/);
    assert.match(welcomeScreen, /await updateRemoteChatName\(characterId,\s*committedFileName\)/);
    assert.doesNotMatch(welcomeScreen, /await updateRemoteChatName\(characterId,\s*newName\)/);
    assert.match(chatCommand, /pub async fn rename_chat\([\s\S]*\)\s*->\s*Result<String, CommandError>/);
    assert.match(groupCommand, /pub async fn rename_group_chat\([\s\S]*\)\s*->\s*Result<String, CommandError>/);
    assert.match(chatService, /pub async fn rename_chat\(&self, dto: RenameChatDto\)\s*->\s*Result<String, ApplicationError>/);
    assert.match(groupService, /pub async fn rename_group_chat\([\s\S]*dto: RenameGroupChatDto,[\s\S]*\)\s*->\s*Result<String, ApplicationError>/);
});
