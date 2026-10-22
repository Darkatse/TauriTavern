import assert from 'node:assert/strict';
import test from 'node:test';

import { payloadToCreateCharacterDto } from '../src/tauri/main/services/characters/character-create-mapper.js';

test('character create payload preserves exact primary lorebook name', () => {
    const dto = payloadToCreateCharacterDto({
        ch_name: 'Alice',
        description: 'desc',
        first_mes: 'hello',
        world: ' Lore ',
        extensions: '{}',
    });

    assert.equal(dto.extensions.world, ' Lore ');
    assert.equal(dto.primary_lorebook, ' Lore ');
});
