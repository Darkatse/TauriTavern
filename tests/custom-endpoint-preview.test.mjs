import assert from 'node:assert/strict';
import test from 'node:test';

import { getCustomEndpointPreview } from '../src/scripts/tauritavern/custom-endpoint.js';

test('Custom Gemini preview follows version, model and streaming URL rules without credentials', () => {
    for (const [base, model, stream, expected] of [
        [' https://example.test/ ', 'gemini-test', false, 'https://example.test/v1beta/models/gemini-test:generateContent'],
        ['https://example.test/proxy/v1/', 'models/gemini-test', true, 'https://example.test/proxy/v1/models/gemini-test:streamGenerateContent?alt=sse'],
        ['https://example.test/v1beta', ' gemini-test ', true, 'https://example.test/v1beta/models/gemini-test:streamGenerateContent?alt=sse'],
        ['https://example.test/proxy', '', false, 'https://example.test/proxy/v1beta/models/<Model>:generateContent'],
    ]) {
        const { url } = getCustomEndpointPreview({
            custom_api_format: 'gemini_generate_content', custom_url: base,
            custom_model: model, stream_openai: stream,
        });
        assert.equal(url, expected);
    }
});
