import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerProviderRoutes } from '../src/tauri/main/routes/provider-routes.js';
import { registerSettingsRoutes } from '../src/tauri/main/routes/settings-routes.js';

const SECRET_BACKED_PROVIDER_METADATA_COMMANDS = [
    'get_openrouter_credits',
    'get_nanogpt_credits',
    'get_siliconflow_embedding_models',
    'get_workers_ai_embedding_models',
    'get_workers_ai_multimodal_models',
];

let vectorsRuntimeSourcePromise;

function createProviderRouter(context) {
    const router = createRouteRegistry();
    registerProviderRoutes(router, context, { jsonResponse });
    return router;
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
                let end = i + 1;
                if (source[end] === ';') {
                    end++;
                }
                return source.slice(start, end);
            }
        }
    }

    assert.fail(`Unterminated declaration: ${marker}`);
}

async function getVectorsRuntimeSource() {
    if (!vectorsRuntimeSourcePromise) {
        vectorsRuntimeSourcePromise = readFile(new URL('../src/scripts/extensions/vectors/index.js', import.meta.url), 'utf8')
            .then(source => [
                extractDeclaration(source, 'const remoteEmbeddingEndpoints = {'),
                extractDeclaration(source, 'function toggleSettings()'),
                extractDeclaration(source, 'async function loadRemoteEmbeddingModels(source)'),
            ].join('\n\n'));
    }

    return vectorsRuntimeSourcePromise;
}

class FakeJQueryElement {
    constructor(selector) {
        this.selector = selector;
        this.options = [];
        this.value = '';
        this.visible = null;
    }

    empty() {
        this.options = [];
        return this;
    }

    append(option) {
        this.options.push({
            value: option.value,
            text: option.text,
        });
        return this;
    }

    val(value) {
        if (arguments.length === 0) {
            return this.value;
        }

        this.value = value;
        return this;
    }

    toggle(value) {
        this.visible = Boolean(value);
        return this;
    }
}

async function createVectorsRuntimeHarness({
    oaiSettings = {},
    settings: settingsPatch = {},
    responses = {},
} = {}) {
    const source = await getVectorsRuntimeSource();
    const elements = new Map();
    const fetchCalls = [];
    const savedSettings = [];
    const toastErrors = [];
    const consoleErrors = [];

    const settings = {
        source: 'workers_ai',
        enabled_files: false,
        enabled_chats: false,
        enabled_world_info: false,
        workers_ai_model: '',
        siliconflow_model: '',
        electronhub_model: '',
        ...settingsPatch,
    };
    const extension_settings = { vectors: {} };
    const oai_settings = {
        siliconflow_endpoint: 'cn',
        workers_ai_account_id: 'account-123',
        ...oaiSettings,
    };

    function $(selector) {
        const key = String(selector);
        if (!elements.has(key)) {
            elements.set(key, new FakeJQueryElement(key));
        }
        return elements.get(key);
    }

    const fetch = async (url, init = {}) => {
        fetchCalls.push({ url, init });
        const response = responses[url];
        if (response instanceof Error) {
            throw response;
        }
        if (!response) {
            return { ok: false, status: 404, json: async () => ({}) };
        }
        return {
            ok: response.ok !== false,
            status: response.status ?? 200,
            json: async () => response.json,
        };
    };

    const factory = new Function(
        'settings',
        'extension_settings',
        'oai_settings',
        'getRequestHeaders',
        'saveSettingsDebounced',
        '$',
        'document',
        'fetch',
        'toastr',
        'console',
        'vectorApiRequiresUrl',
        'loadWebLlmModels',
        `${source}\nreturn { remoteEmbeddingEndpoints, toggleSettings, loadRemoteEmbeddingModels };`,
    );

    const runtime = factory(
        settings,
        extension_settings,
        oai_settings,
        () => ({ 'x-test': 'headers' }),
        () => savedSettings.push({ ...settings }),
        $,
        { createElement: tagName => ({ tagName: String(tagName).toUpperCase(), value: '', text: '' }) },
        fetch,
        { error: (message, title, options) => toastErrors.push({ message, title, options }) },
        { error: (...args) => consoleErrors.push(args) },
        ['llamacpp', 'vllm', 'ollama', 'koboldcpp'],
        () => {},
    );

    return {
        runtime,
        settings,
        extension_settings,
        elements,
        fetchCalls,
        savedSettings,
        toastErrors,
        consoleErrors,
    };
}

test('provider secret mutations invalidate secret-backed metadata caches', async () => {
    const router = createRouteRegistry();
    const invokes = [];
    const invalidations = [];
    const context = {
        safeInvoke: async (command, args) => {
            invokes.push({ command, args });
            return command === 'write_secret' ? 'secret-id' : undefined;
        },
        invalidateInvokeAll: (command) => {
            invalidations.push(command);
        },
    };

    registerSettingsRoutes(router, context, { jsonResponse });

    for (const request of [
        {
            path: '/api/secrets/write',
            body: { key: 'api_key_openrouter', value: 'new-key', label: 'OpenRouter' },
        },
        {
            path: '/api/secrets/delete',
            body: { key: 'api_key_nanogpt', id: 'secret-id' },
        },
        {
            path: '/api/secrets/rotate',
            body: { key: 'api_key_workers_ai', id: 'secret-id' },
        },
    ]) {
        invalidations.length = 0;
        const response = await router.handle({ method: 'POST', ...request });

        assert.ok(response);
        assert.equal(response.status, 200);
        assert.deepEqual(invalidations, SECRET_BACKED_PROVIDER_METADATA_COMMANDS);
    }

    assert.deepEqual(invokes.map(call => call.command), ['write_secret', 'delete_secret', 'rotate_secret']);
});

test('provider metadata routes map upstream request bodies to Tauri invoke DTOs', async () => {
    const calls = [];
    const router = createProviderRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            switch (command) {
                case 'get_nanogpt_model_providers':
                    return { supportsProviderSelection: true, providers: ['chutes'] };
                case 'get_openrouter_credits':
                case 'get_nanogpt_credits':
                    return {};
                default:
                    return [];
            }
        },
    });

    const requests = [
        {
            path: '/api/openrouter/models/providers',
            body: { model: 'openai/gpt-4o' },
            expected: { command: 'get_openrouter_model_providers', args: { dto: { model: 'openai/gpt-4o' } } },
        },
        {
            path: '/api/nanogpt/models/providers',
            body: { model: 'gpt-4o-mini' },
            expected: { command: 'get_nanogpt_model_providers', args: { dto: { model: 'gpt-4o-mini' } } },
        },
        {
            path: '/api/openai/siliconflow/models/embedding',
            body: { siliconflow_endpoint: 'cn' },
            expected: { command: 'get_siliconflow_embedding_models', args: { dto: { siliconflow_endpoint: 'cn' } } },
        },
        {
            path: '/api/openai/workers-ai/models/embedding',
            body: { workers_ai_account_id: 'account-id' },
            expected: { command: 'get_workers_ai_embedding_models', args: { dto: { workers_ai_account_id: 'account-id' } } },
        },
        {
            path: '/api/backends/chat-completions/multimodal-models/workers_ai',
            body: { workers_ai_account_id: 'account-id' },
            expected: { command: 'get_workers_ai_multimodal_models', args: { dto: { workers_ai_account_id: 'account-id' } } },
        },
    ];

    for (const request of requests) {
        const response = await router.handle({
            method: 'POST',
            path: request.path,
            body: request.body,
        });

        assert.ok(response);
        assert.equal(response.status, 200);
    }

    assert.deepEqual(calls, requests.map(request => request.expected));
});

test('provider metadata route errors stay visible to callers', async () => {
    const router = createProviderRouter({
        safeInvoke: async () => {
            throw new Error('provider unavailable');
        },
    });

    await assert.rejects(
        router.handle({
            method: 'POST',
            path: '/api/openai/workers-ai/models/embedding',
            body: { workers_ai_account_id: 'account-id' },
        }),
        /provider unavailable/,
    );
});

test('vectors remote embedding loader posts provider metadata bodies and persists first model', async () => {
    const harness = await createVectorsRuntimeHarness({
        responses: {
            '/api/openai/workers-ai/models/embedding': {
                json: [
                    { id: '@cf/baai/bge-m3' },
                    { id: '@cf/baai/bge-small-en-v1.5' },
                ],
            },
            '/api/openai/siliconflow/models/embedding': {
                json: [
                    { id: 'Qwen/Qwen3-Embedding-0.6B' },
                ],
            },
        },
    });

    await harness.runtime.loadRemoteEmbeddingModels('workers_ai');
    await harness.runtime.loadRemoteEmbeddingModels('siliconflow');

    assert.deepEqual(harness.fetchCalls.map(call => ({
        url: call.url,
        method: call.init.method,
        headers: call.init.headers,
        body: JSON.parse(call.init.body),
    })), [
        {
            url: '/api/openai/workers-ai/models/embedding',
            method: 'POST',
            headers: { 'x-test': 'headers' },
            body: { workers_ai_account_id: 'account-123' },
        },
        {
            url: '/api/openai/siliconflow/models/embedding',
            method: 'POST',
            headers: { 'x-test': 'headers' },
            body: { siliconflow_endpoint: 'cn' },
        },
    ]);

    assert.deepEqual(harness.elements.get('#vectors_workers_ai_model').options, [
        { value: '@cf/baai/bge-m3', text: '@cf/baai/bge-m3' },
        { value: '@cf/baai/bge-small-en-v1.5', text: '@cf/baai/bge-small-en-v1.5' },
    ]);
    assert.equal(harness.settings.workers_ai_model, '@cf/baai/bge-m3');
    assert.equal(harness.extension_settings.vectors.workers_ai_model, '@cf/baai/bge-m3');
    assert.equal(harness.elements.get('#vectors_workers_ai_model').value, '@cf/baai/bge-m3');
    assert.equal(harness.settings.siliconflow_model, 'Qwen/Qwen3-Embedding-0.6B');
    assert.equal(harness.savedSettings.length, 2);
});

test('vectors remote embedding loader applies upstream model shape configuration', async () => {
    const harness = await createVectorsRuntimeHarness({
        responses: {
            '/api/openai/electronhub/models': {
                json: [
                    { id: 'chat-only', name: 'Chat Only', endpoints: ['/v1/chat/completions'] },
                    { id: 'embed-model', name: 'Embeddings', endpoints: ['/v1/embeddings'] },
                ],
            },
        },
    });

    await harness.runtime.loadRemoteEmbeddingModels('electronhub');

    assert.deepEqual(harness.elements.get('#vectors_electronhub_model').options, [
        { value: 'embed-model', text: 'Embeddings' },
    ]);
    assert.equal(harness.settings.electronhub_model, 'embed-model');
    assert.equal(harness.extension_settings.vectors.electronhub_model, 'embed-model');
});

test('vectors source toggle exposes remote model failures without clearing current state', async () => {
    const harness = await createVectorsRuntimeHarness({
        settings: {
            source: 'workers_ai',
            workers_ai_model: '@cf/existing',
        },
        responses: {
            '/api/openai/workers-ai/models/embedding': {
                ok: false,
                status: 401,
                json: { error: 'unauthorized' },
            },
        },
    });
    const select = new FakeJQueryElement('#vectors_workers_ai_model');
    harness.elements.set('#vectors_workers_ai_model', select);
    select.options = [{ value: '@cf/existing', text: '@cf/existing' }];
    select.value = '@cf/existing';

    harness.runtime.toggleSettings();
    await new Promise(resolve => setTimeout(resolve, 0));

    assert.equal(harness.elements.get('#workers_ai_vectorsModel').visible, true);
    assert.equal(harness.elements.get('#siliconflow_vectorsModel').visible, false);
    assert.deepEqual(harness.fetchCalls.map(call => call.url), ['/api/openai/workers-ai/models/embedding']);
    assert.deepEqual(select.options, [{ value: '@cf/existing', text: '@cf/existing' }]);
    assert.equal(select.value, '@cf/existing');
    assert.deepEqual(harness.toastErrors, [
        {
            message: 'HTTP 401',
            title: 'Vector model list failed',
            options: { preventDuplicates: true },
        },
    ]);
    assert.equal(harness.consoleErrors.length, 1);
    assert.equal(harness.savedSettings.length, 0);
});

test('unrelated secret mutations leave provider metadata caches intact', async () => {
    const router = createRouteRegistry();
    const invalidations = [];
    const context = {
        safeInvoke: async () => 'secret-id',
        invalidateInvokeAll: (command) => {
            invalidations.push(command);
        },
    };

    registerSettingsRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/secrets/write',
        body: { key: 'api_key_openai', value: 'new-key' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(invalidations, []);
});
