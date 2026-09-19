import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCaptionRoutes } from '../src/tauri/main/routes/caption-routes.js';

const IMAGE = 'data:image/png;base64,AAAA';

function createCaptionRouter(invokeChatCompletion) {
    const router = createRouteRegistry();
    registerCaptionRoutes(router, { jsonResponse, invokeChatCompletion });
    return router;
}

test('native caption routes delegate supported providers to quiet multimodal chat completion', async () => {
    const calls = [];
    const router = createCaptionRouter(async (dto, signal) => {
        calls.push({ dto, signal });
        return {
            choices: [{ message: { content: `  ${dto.chat_completion_source} caption  ` } }],
        };
    });
    const signal = {};
    const cases = [
        ['openai', '/api/openai/caption-image', 'openai'],
        ['openrouter', '/api/openai/caption-image', 'openrouter'],
        ['custom', '/api/openai/caption-image', 'custom'],
        ['anthropic', '/api/anthropic/caption-image', 'claude'],
        ['google', '/api/google/caption-image', 'makersuite'],
        ['vertexai', '/api/google/caption-image', 'vertexai'],
        ['cohere', '/api/openai/caption-image', 'cohere'],
        ['groq', '/api/openai/caption-image', 'groq'],
        ['moonshot', '/api/openai/caption-image', 'moonshot'],
        ['nanogpt', '/api/openai/caption-image', 'nanogpt'],
        ['chutes', '/api/openai/caption-image', 'chutes'],
        ['workers_ai', '/api/openai/caption-image', 'workers_ai'],
        ['zai', '/api/openai/caption-image', 'zai'],
    ];

    for (const [api, path, source] of cases) {
        const response = await router.handle({
            method: 'POST',
            path,
            body: {
                api,
                image: IMAGE,
                prompt: 'Describe this image.',
                model: `${api}-model`,
                reverse_proxy: 'https://proxy.example/v1',
                proxy_password: 'proxy-secret',
                server_url: 'https://custom.example/v1',
                custom_include_headers: { 'X-Test': 'yes' },
                custom_include_body: { temperature: 0.2 },
                custom_exclude_body: ['seed'],
                vertexai_auth_mode: 'full',
                vertexai_region: 'us-central1',
                vertexai_express_project_id: 'project-id',
                zai_endpoint: 'coding',
                workers_ai_account_id: 'account-id',
                moonshot_endpoint: 'cn',
            },
            init: { signal },
        });

        assert.equal(response.status, 200, api);
        assert.deepEqual(await response.json(), { caption: `${source} caption` }, api);
        const call = calls.at(-1);
        assert.equal(call.signal, signal, api);
        assert.equal(call.dto.chat_completion_source, source, api);
        assert.equal(call.dto.model, `${api}-model`, api);
        assert.equal(call.dto.type, 'quiet', api);
        assert.equal(call.dto.stream, false, api);
        assert.equal(call.dto.messages[0].role, 'user', api);
        assert.equal(call.dto.messages[0].content.length, 2, api);
    }

    const bySource = Object.fromEntries(calls.map(({ dto }) => [dto.chat_completion_source, dto]));
    assert.equal(bySource.claude.messages[0].content[0].type, 'image_url');
    assert.equal(bySource.claude.max_tokens, 4096);
    assert.equal(bySource.custom.custom_url, 'https://custom.example/v1');
    assert.equal(bySource.custom.custom_api_format, 'openai_compat');
    assert.deepEqual(bySource.custom.custom_include_headers, { 'X-Test': 'yes' });
    assert.equal(bySource.vertexai.vertexai_auth_mode, 'full');
    assert.equal(bySource.vertexai.vertexai_region, 'us-central1');
    assert.equal(bySource.workers_ai.workers_ai_account_id, 'account-id');
    assert.equal(bySource.moonshot.moonshot_endpoint, 'cn');
    assert.equal(bySource.zai.zai_endpoint, 'coding');
    assert.equal(bySource.zai.max_tokens, 4096);
});

test('native caption routes reject unsupported sources and invalid media before invoking Rust', async () => {
    let invokeCount = 0;
    const router = createCaptionRouter(async () => {
        invokeCount += 1;
        return { choices: [{ message: { content: 'caption' } }] };
    });

    for (const path of [
        '/api/extra/caption',
        '/api/horde/caption-image',
        '/api/backends/text-completions/ollama/caption-image',
    ]) {
        const response = await router.handle({ method: 'POST', path, body: {} });
        assert.equal(response.status, 501, path);
    }

    const unsupported = await router.handle({
        method: 'POST',
        path: '/api/openai/caption-image',
        body: { api: 'mistral', image: IMAGE, prompt: 'Describe.', model: 'pixtral' },
    });
    assert.equal(unsupported.status, 501);
    assert.match((await unsupported.json()).message, /mistral/);

    const invalidImage = await router.handle({
        method: 'POST',
        path: '/api/openai/caption-image',
        body: { api: 'openai', image: 'AAAA', prompt: 'Describe.', model: 'gpt-4o' },
    });
    assert.equal(invalidImage.status, 400);

    const unsupportedVideo = await router.handle({
        method: 'POST',
        path: '/api/openai/caption-image',
        body: { api: 'openai', image: 'data:video/mp4;base64,AAAA', prompt: 'Describe.', model: 'gpt-4o' },
    });
    assert.equal(unsupportedVideo.status, 400);

    const wrongRoute = await router.handle({
        method: 'POST',
        path: '/api/openai/caption-image',
        body: { api: 'anthropic', image: IMAGE, prompt: 'Describe.', model: 'claude' },
    });
    assert.equal(wrongRoute.status, 400);
    assert.equal(invokeCount, 0);
});

test('native caption routes preserve video parts and fail on missing or failed model output', async () => {
    let nextResult = {
        choices: [{ message: { content: 'video caption' } }],
    };
    const calls = [];
    const router = createCaptionRouter(async (dto) => {
        calls.push(dto);
        if (nextResult instanceof Error) {
            throw nextResult;
        }
        return nextResult;
    });
    const request = {
        method: 'POST',
        path: '/api/openai/caption-image',
        body: {
            api: 'zai',
            image: 'data:video/mp4;base64,AAAA',
            prompt: 'Describe this video.',
            model: 'glm-4v',
        },
    };

    const success = await router.handle(request);
    assert.equal(success.status, 200);
    assert.equal(calls[0].messages[0].content[1].type, 'video_url');

    nextResult = { choices: [{ message: { content: '' } }] };
    const empty = await router.handle(request);
    assert.equal(empty.status, 502);
    assert.match((await empty.json()).error.message, /missing assistant text/);

    nextResult = new Error('provider unavailable');
    const failed = await router.handle(request);
    assert.equal(failed.status, 502);
    assert.equal((await failed.json()).error.message, 'provider unavailable');
});
