import assert from 'node:assert/strict';
import test from 'node:test';
import { createPresetConnectionKeeper } from '../src/scripts/extensions/connection-manager/preset-connection.js';

const CONNECTION_KEYS = ['chat_completion_source', 'custom_url', 'custom_model'];

/** Body the live settings produce while a saved model (OpenAI) owns the connection. */
const liveBody = () => ({ temperature: 0.7, chat_completion_source: 'openai', custom_url: '', custom_model: '' });
const storedPreset = () => ({ temperature: 1, chat_completion_source: 'custom', custom_url: 'https://a.example/v1', custom_model: 'a-1' });
const storedConnection = { chat_completion_source: 'custom', custom_url: 'https://a.example/v1', custom_model: 'a-1' };

test('renaming a preset keeps its own connection, not the placeholder or the live model', () => {
    const keeper = createPresetConnectionKeeper(CONNECTION_KEYS);
    keeper.renaming('Renamed', storedPreset());

    // Upstream stores `{}` (plus extensions) under the new name, loads it, then overwrites it.
    const body = liveBody();
    keeper.saving({ name: 'Renamed', preset: body, previous: { extensions: {} }, loaded: { extensions: {} }, owned: true });
    assert.deepEqual(body, { temperature: 0.7, ...storedConnection });

    // The capture served that save; a later overwrite follows the stored entry again.
    const later = liveBody();
    keeper.saving({ name: 'Renamed', preset: later, previous: { temperature: 0.7 }, loaded: null, owned: true });
    assert.deepEqual(later, { temperature: 0.7 });
});

test('a rename capture does not outlive the save that follows it', () => {
    const keeper = createPresetConnectionKeeper(CONNECTION_KEYS);
    keeper.renaming('Renamed', storedPreset());
    keeper.saving({ name: 'Other', preset: liveBody(), previous: null, loaded: null, owned: false });

    const body = liveBody();
    keeper.saving({ name: 'Renamed', preset: body, previous: { chat_completion_source: 'claude' }, loaded: null, owned: true });
    assert.deepEqual(body, { temperature: 0.7, chat_completion_source: 'claude' });
});

test('overwriting keeps the stored connection; saving as a new name keeps the source preset\'s', () => {
    const keeper = createPresetConnectionKeeper(CONNECTION_KEYS);

    const overwrite = liveBody();
    keeper.saving({ name: 'A', preset: overwrite, previous: storedPreset(), loaded: storedPreset(), owned: true });
    assert.deepEqual(overwrite, { temperature: 0.7, ...storedConnection });

    const saveAs = liveBody();
    keeper.saving({ name: 'A copy', preset: saveAs, previous: null, loaded: storedPreset(), owned: true });
    assert.deepEqual(saveAs, { temperature: 0.7, ...storedConnection });

    const orphan = liveBody();
    keeper.saving({ name: 'B', preset: orphan, previous: null, loaded: null, owned: true });
    assert.deepEqual(orphan, { temperature: 0.7 });
});
