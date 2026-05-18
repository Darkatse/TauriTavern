import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

function extractArrayDeclaration(source, name) {
    const marker = `const ${name} = [`;
    const start = source.indexOf(marker);
    assert.notEqual(start, -1, `Missing ${name}`);
    const bodyStart = source.indexOf('[', start);
    let depth = 0;
    let quote = '';

    for (let i = bodyStart; i < source.length; i++) {
        const char = source[i];
        const next = source[i + 1];
        if (quote) {
            if (char === '\\') {
                i++;
            } else if (char === quote) {
                quote = '';
            }
            continue;
        }
        if (char === '/' && next === '/') {
            const lineEnd = source.indexOf('\n', i + 2);
            i = lineEnd === -1 ? source.length : lineEnd;
            continue;
        }
        if (char === '/' && next === '*') {
            const commentEnd = source.indexOf('*/', i + 2);
            i = commentEnd === -1 ? source.length : commentEnd + 1;
            continue;
        }
        if (char === '"' || char === '\'' || char === '`') {
            quote = char;
            continue;
        }
        if (char === '[') {
            depth++;
        } else if (char === ']') {
            depth--;
            if (depth === 0) {
                return source.slice(bodyStart, i + 1);
            }
        }
    }

    assert.fail(`Unterminated ${name}`);
}

function extractProviderIds(source, name) {
    const declaration = extractArrayDeclaration(source, name);
    if (name === 'OPENROUTER_PROVIDERS') {
        return [...declaration.matchAll(/'([^']+)'/g)].map(match => match[1]);
    }

    return [...declaration.matchAll(/'?id'?:\s*'([^']+)'/g)].map(match => match[1]);
}

test('Welcome screen F sync adds configurable recent chats without dropping Tauri local affordances', async () => {
    const [source, template] = await Promise.all([
        readProjectFile('src/scripts/welcome-screen.js'),
        readProjectFile('src/scripts/templates/welcomePanel.html'),
    ]);

    assert.match(source, /import \{ clamp, flashHighlight, isElementInViewport, sortMoments, timestampToMoment \} from '\.\/utils\.js';/);
    assert.match(source, /const recentChatsSettingsKey = 'recentChatsSettings';/);
    assert.match(source, /const DEFAULT_MAX_DISPLAYED = 15;/);
    assert.match(source, /const DEFAULT_COLLAPSED_DISPLAYED = 3;/);
    assert.match(source, /function getRecentChatsSettings\(\)/);
    assert.match(source, /function saveRecentChatsSettings\(settings\)/);
    assert.match(source, /await openRecentChatsSettingsPopup\(\)/);
    assert.match(source, /await callGenericPopup\(t`Recent Chats Settings`, POPUP_TYPE\.CONFIRM, null, \{/);
    assert.match(source, /label: t`Max recent chats`/);
    assert.match(source, /label: t`Collapsed recent chats`/);
    assert.match(source, /body: JSON\.stringify\(\{ max: settings\.maxDisplayed, pinned: PinnedChatsManager\.getAll\(\) \}\)/);
    assert.match(source, /chat\.hidden = index >= settings\.collapsedDisplayed;/);
    assert.match(source, /focusChatInput\(ChatInputFocusIntent\.NAVIGATION\)/);
    assert.match(source, /const committedFileName = await renameGroupOrCharacterChat/);
    assert.match(source, /await updateRemoteChatName\(characterId, committedFileName\)/);
    assert.match(source, /Tauri thumbnail bridge can normalize \/thumbnail URLs/);

    assert.match(template, /data-i18n="\[alt\]SillyTavern Logo"/);
    assert.match(template, /class="mes_button recentChatsSettings" title="Recent chats settings"/);
    assert.match(template, /href="https:\/\/tauritavern\.github\.io"/);
    assert.match(template, /href="https:\/\/github\.com\/Darkatse\/TauriTavern"/);
    assert.match(template, /id="tt-welcome-discord-link"/);
});

test('NanoGPT provider selector F sync uses upstream Select2 affordances', async () => {
    const source = await readProjectFile('src/scripts/textgen-models.js');

    assert.match(source, /const nanoGptProvidersSelect = \$\('#nanogpt_provider'\);/);
    assert.match(source, /nanoGptProvidersSelect\.select2\(\{\s*sorter: data => data\.sort\(\(a, b\) => a\.text\.localeCompare\(b\.text\)\),\s*placeholder: t`Select providers\. No selection = all providers\.`,\s*searchInputPlaceholder: t`Search providers\.\.\.`,\s*searchInputCssClass: 'text_pole',\s*width: '100%',\s*allowClear: true,\s*\}\);/);
});

test('Textgen provider constants stay aligned with the 1.18 upstream reference', async (t) => {
    let upstream;
    try {
        upstream = await readProjectFile('sillytavern-1.18.0/public/scripts/textgen-models.js');
    } catch {
        t.skip('sillytavern-1.18.0 symlink is not available');
        return;
    }

    const source = await readProjectFile('src/scripts/textgen-models.js');
    for (const name of ['OPENROUTER_PROVIDERS', 'NANOGPT_PROVIDERS']) {
        assert.deepEqual(extractProviderIds(source, name), extractProviderIds(upstream, name), name);
    }
});

test('Caption F sync fails explicitly in native routes instead of exposing unsupported static model choices', async () => {
    globalThis.window = globalThis.window || {};
    const [{ registerAiRoutes }, routesIndex, settings, captionSource, shared, stableDiffusion] = await Promise.all([
        import('../src/tauri/main/routes/ai-routes.js'),
        readProjectFile('src/tauri/main/routes/index.js'),
        readProjectFile('src/scripts/extensions/caption/settings.html'),
        readProjectFile('src/scripts/extensions/caption/index.js'),
        readProjectFile('src/scripts/extensions/shared.js'),
        readProjectFile('src/scripts/extensions/stable-diffusion/index.js'),
    ]);

    const router = createRouteRegistry();
    registerAiRoutes(router, {
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called for caption unavailable routes');
        },
    }, { jsonResponse });

    for (const route of [
        '/api/extra/caption',
        '/api/horde/caption-image',
        '/api/openai/caption-image',
        '/api/google/caption-image',
        '/api/anthropic/caption-image',
        '/api/backends/text-completions/ollama/caption-image',
    ]) {
        assert.equal(router.canHandle('POST', route), true, route);
        const response = await router.handle({ method: 'POST', path: route, body: {} });
        assert.equal(response.status, 501, route);
        const payload = await response.json();
        assert.equal(payload.error, true, route);
        assert.equal(payload.message, 'Image captioning is not implemented in the TauriTavern native backend.', route);
    }

    assert.ok(routesIndex.indexOf('registerAiRoutes(router, context, responses);') < routesIndex.indexOf('registerProviderRoutes(router, context, responses);'));
    assert.match(shared, /typeof data\.message === 'string' && data\.message\.trim\(\)/);
    assert.match(shared, /typeof data\.error === 'string' && data\.error\.trim\(\)/);
    assert.doesNotMatch(shared, /data\.message \|\| data\.error \|\| text/);
    assert.match(shared, /export const NATIVE_CAPTION_UNAVAILABLE_MESSAGE = 'Image captioning is not implemented in the TauriTavern native backend\.';/);
    assert.match(shared, /if \(globalThis\.__TAURI_RUNNING__ === true\) \{\s*throw new Error\(NATIVE_CAPTION_UNAVAILABLE_MESSAGE\);\s*\}/);
    assert.match(shared, /throw new Error\(await getCaptionErrorMessage\(apiResult\)\)/);
    assert.match(captionSource, /import \{ getMultimodalCaption, NATIVE_CAPTION_UNAVAILABLE_MESSAGE \} from '\.\.\/shared\.js';/);
    assert.match(captionSource, /globalThis\.__TAURI_RUNNING__ === true && \['local', 'horde'\]\.includes\(extension_settings\.caption\.source\)/);
    assert.match(captionSource, /globalThis\.__TAURI_RUNNING__ === true && \['local', 'horde', 'multimodal'\]\.includes\(settings\.source\)/);
    assert.match(captionSource, /toastr\.error\(unavailableReason \|\| 'Choose other captioning source in the extension settings\.', 'Captioning is not available'\)/);
    assert.match(stableDiffusion, /const errorMessage = error\?\.message \|\| String\(error\) \|\| 'Multimodal captioning failed\.';/);
    assert.match(stableDiffusion, /toastr\.error\(errorMessage, 'Image Generation'\)/);
    assert.match(stableDiffusion, /throw new Error\(errorMessage, \{ cause: error \}\)/);
    for (const model of [
        'gpt-5.5',
        'gemini-3.1-flash-lite-preview',
        'gemini-3.1-flash-image-preview',
        'gemma-4-31b-it',
        'glm-5v-turbo',
    ]) {
        assert.equal(settings.includes(model), false, model);
    }
});

test('F sync locale keys cover welcome settings and upstream provider placeholders', async () => {
    const [zhCn, zhTw] = await Promise.all([
        readProjectFile('src/locales/zh-cn.json').then(JSON.parse),
        readProjectFile('src/locales/zh-tw.json').then(JSON.parse),
    ]);

    assert.equal(zhCn['SillyTavern Logo'], 'SillyTavern 徽标');
    assert.equal(zhCn['Recent chats settings'], '最近聊天设置');
    assert.equal(zhCn['Recent Chats Settings'], '最近聊天设置');
    assert.equal(zhCn['Max recent chats'], '最多最近聊天数');
    assert.equal(zhCn['Collapsed recent chats'], '折叠时显示的最近聊天数');
    assert.equal(zhCn['Select providers. No selection = all providers.'], '选择供应商。未选择 = 所有供应商。');
    assert.equal(zhCn['Search providers...'], '搜索提供商...');
    assert.equal(zhCn['Tool Call Recurse Limit'], '工具调用递归限制');
    assert.equal(zhCn['Interleaved Thinking'], '交错思维');
    assert.equal(zhCn['Since Last User Message'], '自上一条用户消息起');
    assert.equal(zhCn['Active Tool Chain'], '活动工具链');
    assert.equal(zhCn['Enable "Request model reasoning" to use Interleaved Thinking.'], '启用“请求思维链”以使用交错思维。');

    assert.equal(zhTw['SillyTavern Logo'], 'SillyTavern 標誌');
    assert.equal(zhTw['Recent chats settings'], '最近聊天設定');
    assert.equal(zhTw['Recent Chats Settings'], '最近聊天設定');
    assert.equal(zhTw['Max recent chats'], '最多最近聊天數');
    assert.equal(zhTw['Collapsed recent chats'], '摺疊時顯示的最近聊天數');
    assert.equal(zhTw['Select providers. No selection = all providers.'], '選擇供應商。未選擇＝所有供應商。');
    assert.equal(zhTw['Search providers...'], '搜尋供應商⋯');
    assert.equal(zhTw['Tool Call Recurse Limit'], '工具呼叫遞迴限制');
    assert.equal(zhTw['Interleaved Thinking'], '交錯思維');
    assert.equal(zhTw['Since Last User Message'], '自上一則使用者訊息起');
    assert.equal(zhTw['Active Tool Chain'], '使用中的工具鏈');
    assert.equal(zhTw['Enable "Request model reasoning" to use Interleaved Thinking.'], '啟用「請求模型思維鏈」以使用交錯思維。');
});
