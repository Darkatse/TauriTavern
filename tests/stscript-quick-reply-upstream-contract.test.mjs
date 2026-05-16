import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function extractCommandNames(source) {
    const names = [];
    const pattern = /SlashCommand(?:Parser)?\.addCommandObject\(SlashCommand\.fromProps\(\{[\s\S]*?\bname:\s*['"]([^'"]+)['"]/g;
    let match;
    while ((match = pattern.exec(source))) {
        names.push(match[1]);
    }
    return [...new Set(names)].sort();
}

test('root slash commands include SillyTavern 1.18 P8 command surface and local commands', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');
    const names = extractCommandNames(source);

    for (const name of ['regenerate', 'swipe', 'pm-render', 'array-wrap', 'array-unwrap', 'llmlog', 'custom-api-format']) {
        assert.ok(names.includes(name), `missing slash command: ${name}`);
    }

    assert.match(source, /import\s+\{\s*registerActionLoaderSlashCommands\s*\}\s+from\s+'\.\/action-loader-slashcommands\.js';/);
    assert.match(source, /\bregisterActionLoaderSlashCommands\(\);/);
});

test('local root slash command surface is not missing 1.18 root commands', async (t) => {
    const upstreamPath = path.join(REPO_ROOT, 'sillytavern-1.18.0/public/scripts/slash-commands.js');
    if (!existsSync(upstreamPath)) {
        t.skip('sillytavern-1.18.0 symlink is not available');
        return;
    }

    const [localSource, upstreamSource] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8'),
        readFile(upstreamPath, 'utf8'),
    ]);
    const localNames = extractCommandNames(localSource);
    const upstreamNames = extractCommandNames(upstreamSource);
    const missing = upstreamNames.filter(name => !localNames.includes(name));

    assert.deepEqual(missing, []);
});

test('regenerate and swipe commands preserve generation gate semantics', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');

    assert.match(source, /async function regenerateChatCallback\(args\)[\s\S]*waitUntilCondition\(\(\) => !is_send_press && !is_group_generating/s);
    assert.match(source, /async function regenerateChatCallback\(args\)[\s\S]*regenerateGroup\(\)/s);
    assert.match(source, /async function regenerateChatCallback\(args\)[\s\S]*Generate\('regenerate'\)/s);
    assert.match(source, /async function swipeChatCallback\(args\)[\s\S]*waitUntilCondition\(\(\) => !is_send_press && !is_group_generating/s);
    assert.match(source, /swipe\(null,\s*direction,\s*\{\s*source:\s*SWIPE_SOURCE\.SLASH_COMMAND,\s*repeated:\s*false\s*\}\)/);
});

test('group regenerate returns the generation promise for await semantics', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/group-chats.js'), 'utf8');

    assert.match(
        source,
        /async function regenerateGroup\(\)[\s\S]*return generateGroupWrapper\(false,\s*'normal',\s*\{\s*signal:\s*abortController\.signal\s*\}\);/s,
    );
});

test('common enum providers expose 1.18 STscript helper surface', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands/SlashCommandCommonEnumsProvider.js'), 'utf8');

    assert.match(source, /\bspinner:\s*'♻️'/);
    assert.match(source, /\bstop:\s*'🛑'/);
    assert.match(source, /personas:\s*\(\{\s*allowPersonaKey\s*=\s*false\s*\}\s*=\s*\{\}\)\s*=>\s*\(\)\s*=>/);
    assert.match(source, /backgrounds:\s*\(\)\s*=>\s*Array\.from\(document\.querySelectorAll\('\.bg_example'\)\)/);
    assert.match(source, /connectionProfiles:\s*\(\{\s*includeNone\s*=\s*false\s*\}\s*=\s*\{\}\)\s*=>\s*\(\)\s*=>/);
    assert.match(source, /export const commonEnumMatchProviders\s*=/);
});

test('quick reply keeps Tauri chat input focus contract and 1.18 id assignment', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions/quick-reply/src/QuickReplySet.js'), 'utf8');

    assert.match(source, /import\s+\{\s*ChatInputFocusIntent,\s*focusChatInput,\s*getChatInput\s*\}\s+from\s+'\.\.\/\.\.\/\.\.\/chat-input-focus\.js';/);
    assert.match(source, /const ta = getChatInput\(\);/);
    assert.match(source, /focusChatInput\(ChatInputFocusIntent\.EDITING,\s*\{\s*cursor:\s*'end'\s*\}\);/);
    assert.match(source, /data\.id = this\.idIndex = id \+ 1;/);
});
