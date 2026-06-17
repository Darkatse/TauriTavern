import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';

let connectionManagerRequestServicePromise;

function installBrowserShims() {
    globalThis.window ??= {};
    globalThis.document ??= { visibilityState: 'visible' };
    Object.defineProperty(globalThis, 'localStorage', {
        configurable: true,
        writable: true,
        value: {
            getItem: () => null,
            setItem: () => {},
            removeItem: () => {},
        },
    });
}

function extractDeclaration(source, marker) {
    const start = source.indexOf(marker);
    assert.notEqual(start, -1, `Missing declaration marker: ${marker}`);

    const braceStart = source.indexOf('{', start);
    assert.notEqual(braceStart, -1, `Missing declaration body: ${marker}`);

    let depth = 0;
    let quote = '';

    for (let i = braceStart; i < source.length; i++) {
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

        if (char === '{') {
            depth++;
        } else if (char === '}') {
            depth--;
            if (depth === 0) {
                return source.slice(start, i + 1);
            }
        }
    }

    assert.fail(`Unterminated declaration: ${marker}`);
}

function templateText(strings, ...values) {
    return strings.reduce((text, part, index) => `${text}${part}${values[index] ?? ''}`, '');
}

async function loadConnectionManagerRequestService(deps) {
    if (!connectionManagerRequestServicePromise) {
        connectionManagerRequestServicePromise = readFile(
            new URL('../src/scripts/extensions/shared.js', import.meta.url),
            'utf8',
        ).then((source) => {
            const declaration = extractDeclaration(source, 'export class ConnectionManagerRequestService')
                .replace('export class ConnectionManagerRequestService', 'class ConnectionManagerRequestService');
            return new Function(
                'SillyTavern',
                'proxies',
                'CONNECT_API_MAP',
                'createModelIcon',
                't',
                `${declaration}\nreturn ConnectionManagerRequestService;`,
            );
        });
    }

    const factory = await connectionManagerRequestServicePromise;
    return factory(deps.SillyTavern, deps.proxies, deps.CONNECT_API_MAP, deps.createModelIcon ?? (() => null), templateText);
}

test('chat completion status route forwards secret_id to Rust DTO', async () => {
    installBrowserShims();
    const { registerAiRoutes } = await import('../src/tauri/main/routes/ai-routes.js');
    const router = createRouteRegistry();
    const calls = [];

    registerAiRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { data: [] };
        },
    }, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/status',
        body: {
            chat_completion_source: 'openrouter',
            secret_id: 'profile-secret',
        },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { data: [] });
    assert.equal(calls.length, 1);
    assert.equal(calls[0].command, 'get_chat_completions_status');
    assert.equal(calls[0].args.dto.secret_id, 'profile-secret');
});

test('chat completion status route preserves object custom headers for Rust DTO', async () => {
    installBrowserShims();
    const { registerAiRoutes } = await import('../src/tauri/main/routes/ai-routes.js');
    const router = createRouteRegistry();
    const calls = [];

    registerAiRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { data: [] };
        },
    }, { jsonResponse });

    const customHeaders = {
        'Content-Type': 'application/json',
        Authorization: 'Bearer proxy-secret',
    };

    const response = await router.handle({
        method: 'POST',
        path: '/api/backends/chat-completions/status',
        body: {
            chat_completion_source: 'custom',
            custom_include_headers: customHeaders,
        },
    });

    assert.equal(response.status, 200);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].command, 'get_chat_completions_status');
    assert.deepEqual(calls[0].args.dto.custom_include_headers, customHeaders);
});

test('chat completion status frontend includes active secret id snapshot', async () => {
    const source = await readFile(
        new URL('../src/scripts/openai.js', import.meta.url),
        'utf8',
    );
    const getStatusOpen = extractDeclaration(source, 'async function getStatusOpen');

    assert.match(getStatusOpen, /const secretKey = resolveSecretKey\(\);/);
    assert.match(getStatusOpen, /const activeSecret = Array\.isArray\(secret_state\[secretKey\]\)/);
    assert.match(getStatusOpen, /data\.secret_id = activeSecret\.id;/);
});

test('connection manager applies profiles as a suspended validation batch', async () => {
    const source = await readFile(
        new URL('../src/scripts/extensions/connection-manager/index.js', import.meta.url),
        'utf8',
    );
    const applyConnectionProfile = extractDeclaration(source, 'async function applyConnectionProfile');

    assert.match(applyConnectionProfile, /withConnectionValidationSuspended\('Connection profile application'/);
    assert.match(applyConnectionProfile, /if \(command === 'api-url'\) \{[\s\S]*?commandArgs\.connect = 'false';[\s\S]*?\}/);
    assert.match(applyConnectionProfile, /connectCurrentApi\(\);/);
});

test('connection manager model targets apply only model route fields', async () => {
    const source = await readFile(
        new URL('../src/scripts/extensions/connection-manager/index.js', import.meta.url),
        'utf8',
    );
    const applyModelTarget = extractDeclaration(source, 'async function applyModelTarget');

    assert.match(applyModelTarget, /withConnectionValidationSuspended\('Model target application'/);
    assert.match(applyModelTarget, /requireManagedCommand\('api', target\.api\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('custom-api-format', target\['custom-api-format'\]\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('custom-api-format', 'openai_compat'\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('api-url', target\['api-url'\], \{ connect: 'false', quiet: 'true' \}\)/);
    assert.match(applyModelTarget, /executeManagedCommand\('api-url', '', \{ connect: 'false', quiet: 'true', clear: 'true' \}\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('secret-id', target\.secretRef\.id, \{ key: target\.secretRef\.key, quiet: 'true' \}\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('proxy', NO_PROXY_PRESET\)/);
    assert.match(applyModelTarget, /requireManagedCommand\('model', target\.model, \{ quiet: 'true' \}\)/);
    assert.doesNotMatch(applyModelTarget, /preset|stop-strings|start-reply-with|prompt-post-processing|regex-preset/);
});

test('connection manager preserves legacy profile selection contracts', async () => {
    const source = await readFile(
        new URL('../src/scripts/extensions/connection-manager/index.js', import.meta.url),
        'utf8',
    );
    const makeItemOptionValue = extractDeclaration(source, 'function makeItemOptionValue');
    const setSelectedItemRef = extractDeclaration(source, 'function setSelectedItemRef');
    const normalizeConnectionManagerSettings = extractDeclaration(source, 'function normalizeConnectionManagerSettings');

    assert.match(makeItemOptionValue, /kind === CONNECTION_ITEM_KIND\.PROFILE[\s\S]*?return id;/);
    assert.match(makeItemOptionValue, /kind === CONNECTION_ITEM_KIND\.MODEL_TARGET[\s\S]*?return `\$\{kind\}:\$\{id\}`;/);
    assert.match(setSelectedItemRef, /if \(!ref\) \{[\s\S]*?selectedProfile = null;[\s\S]*?\} else if \(ref\.kind === CONNECTION_ITEM_KIND\.PROFILE\) \{[\s\S]*?selectedProfile = ref\.id;/);
    assert.doesNotMatch(setSelectedItemRef, /selectedProfile = ref\?\.kind/);
    assert.doesNotMatch(normalizeConnectionManagerSettings, /selectedProfile = null/);
});

test('connection manager popup custom actions are result-driven', async () => {
    const source = await readFile(
        new URL('../src/scripts/extensions/connection-manager/index.js', import.meta.url),
        'utf8',
    );

    assert.match(source, /new Popup\(template, POPUP_TYPE\.INPUT, suggestedName/);
    assert.match(source, /popup\.result === CREATE_MODEL_TARGET_RESULT/);
    assert.match(source, /popup\.result === POPUP_RESULT\.CUSTOM1/);
    assert.doesNotMatch(source, /action: \(\) => \{/);
});

test('api-url slash command has explicit clear semantics for model targets', async () => {
    const source = await readFile(
        new URL('../src/scripts/slash-commands.js', import.meta.url),
        'utf8',
    );

    assert.match(source, /async function setApiUrlCallback\(\{ api = null, connect = 'true', quiet = 'false', clear = 'false' \}/);
    assert.match(source, /const isClear = isTrueBoolean\(clear\);/);
    assert.match(source, /\$\('#custom_api_url_text'\)\.val\(''\)\.trigger\('input'\);/);
    assert.match(source, /\$\('#api_url_text'\)\.val\(''\)\.trigger\('input'\);/);
    assert.match(source, /\$\(inputSelector\)\.val\(''\)\.trigger\('input'\);/);
    assert.match(source, /name: 'clear',[\s\S]*?description: t`Clear the current API URL instead of reading it when no URL is provided`/);
});

test('connection manager model target visible strings have zh translations', async () => {
    const keys = [
        'Name cannot be empty.',
        'A model with the same name already exists.',
        'A profile with the same name already exists.',
        'Please provide a name for the new connection profile.',
        'Save Model Only',
        'Save only API, server URL, model, proxy, and secret.',
        'Are you sure you want to delete the selected model?',
        'Saved Object',
        'Model',
        'Model name:',
        'Models',
        'Connection Profiles',
        'Rename and refresh the saved model route from the current connection settings.',
        'Model renamed.',
        'Connection profile reloaded',
        'Model reloaded',
        'Connection profile updated',
        'Model updated',
        'Press "Update" to record them into the profile.',
        'Included settings list updated',
        'Connection profile renamed.',
        'Clear the current API URL instead of reading it when no URL is provided',
    ];
    const locales = [
        ['zh-cn', '../src/locales/zh-cn.json'],
        ['zh-tw', '../src/locales/zh-tw.json'],
    ];

    for (const [localeName, localePath] of locales) {
        const locale = JSON.parse(await readFile(new URL(localePath, import.meta.url), 'utf8'));
        for (const key of keys) {
            assert.ok(Object.hasOwn(locale, key), `${localeName} is missing translation key: ${key}`);
            assert.equal(typeof locale[key], 'string', `${localeName} translation is not a string: ${key}`);
            assert.notEqual(locale[key].length, 0, `${localeName} translation is empty: ${key}`);
        }
    }
});

test('connection manager forwards profile secret id for completion requests', async () => {
    const chatRequests = [];
    const textRequests = [];
    const CONNECT_API_MAP = {
        minimax: { selected: 'openai', source: 'minimax' },
        koboldcpp: { selected: 'textgenerationwebui', type: 'koboldcpp' },
    };
    const context = {
        CONNECT_API_MAP,
        extensionSettings: {
            disabledExtensions: [],
            connectionManager: {
                profiles: [
                    {
                        id: 'chat-profile',
                        api: 'minimax',
                        model: 'MiniMax-M2.7',
                        preset: 'chat-preset',
                        proxy: 'main-proxy',
                        'api-url': 'cn',
                        'prompt-post-processing': 'merge-tools',
                        'custom-api-format': 'claude-messages',
                        'secret-id': 'chat-secret',
                    },
                    {
                        id: 'text-profile',
                        api: 'koboldcpp',
                        model: 'kobold-model',
                        preset: 'text-preset',
                        instruct: 'text-instruct',
                        'api-url': 'https://text.example',
                        'secret-id': 'text-secret',
                    },
                ],
            },
        },
        ChatCompletionService: {
            processRequest: async (...args) => {
                chatRequests.push(args);
                return 'chat-result';
            },
        },
        TextCompletionService: {
            processRequest: async (...args) => {
                textRequests.push(args);
                return 'text-result';
            },
        },
    };
    const ConnectionManagerRequestService = await loadConnectionManagerRequestService({
        SillyTavern: { getContext: () => context },
        proxies: [{ name: 'main-proxy', url: 'https://proxy.example/v1', password: 'proxy-secret' }],
        CONNECT_API_MAP,
    });

    assert.equal(
        await ConnectionManagerRequestService.sendRequest('chat-profile', 'hello', 123, {
            includePreset: false,
            includeInstruct: false,
        }),
        'chat-result',
    );
    assert.equal(
        await ConnectionManagerRequestService.sendRequest('text-profile', 'hi', 77, {
            includePreset: false,
            includeInstruct: false,
        }),
        'text-result',
    );

    assert.equal(chatRequests.length, 1);
    assert.equal(chatRequests[0][0].secret_id, 'chat-secret');
    assert.equal(chatRequests[0][0].chat_completion_source, 'minimax');
    assert.equal(chatRequests[0][0].custom_api_format, 'claude-messages');
    assert.equal(chatRequests[0][0].minimax_endpoint, 'cn');
    assert.deepEqual(chatRequests[0][0].messages, [{ role: 'user', content: 'hello' }]);
    assert.equal(chatRequests[0][0].reverse_proxy, 'https://proxy.example/v1');
    assert.equal(chatRequests[0][0].proxy_password, 'proxy-secret');

    assert.equal(textRequests.length, 1);
    assert.equal(textRequests[0][0].secret_id, 'text-secret');
    assert.equal(textRequests[0][0].api_type, 'koboldcpp');
    assert.equal(textRequests[0][0].api_server, 'https://text.example');
});

test('connection manager fails fast for native Text Completion profiles', async () => {
    const CONNECT_API_MAP = {
        koboldcpp: { selected: 'textgenerationwebui', type: 'koboldcpp' },
    };
    const context = {
        CONNECT_API_MAP,
        extensionSettings: {
            disabledExtensions: [],
            connectionManager: {
                profiles: [{
                    id: 'text-profile',
                    api: 'koboldcpp',
                    model: 'kobold-model',
                    preset: 'text-preset',
                    instruct: 'text-instruct',
                    'api-url': 'https://text.example',
                }],
            },
        },
        TextCompletionService: {
            processRequest: async () => {
                throw new Error('should not call native text completion');
            },
        },
    };
    const ConnectionManagerRequestService = await loadConnectionManagerRequestService({
        SillyTavern: { getContext: () => context },
        proxies: [],
        CONNECT_API_MAP,
    });

    globalThis.__TAURI_RUNNING__ = true;
    try {
        await assert.rejects(
            () => ConnectionManagerRequestService.sendRequest('text-profile', 'hi', 77, {
                includePreset: false,
                includeInstruct: false,
            }),
            (error) => {
                assert.equal(error.message, 'API request failed');
                assert.match(error.cause?.message, /Text Completion profiles are not supported/);
                return true;
            },
        );
    } finally {
        delete globalThis.__TAURI_RUNNING__;
    }
});
