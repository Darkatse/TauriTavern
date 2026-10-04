import test from 'node:test';
import assert from 'node:assert/strict';
import { installCharacterCardsApi } from '../src/tauri/main/api/character-cards.js';

function installHarness({ files, readFile }) {
    const discarded = [];
    globalThis.window = { __TAURITAVERN__: { api: {} } };
    installCharacterCardsApi({
        safeInvoke: async (command, args) => {
            if (command === 'pick_import_files') {
                return files;
            }
            if (command === 'stage_file_discard') {
                discarded.push(args.filePath);
                return;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        createReadableFileStream: async path => new ReadableStream({
            start(controller) {
                controller.enqueue(readFile(path));
                controller.close();
            },
        }),
    });
    return { characterCards: window.__TAURITAVERN__.api.characterCards, discarded };
}

test('native character selection preserves original names and bytes while releasing staged copies', async () => {
    const selected = [
        { path: '/cache/one.json', name: 'Alice%20.json' },
        { path: '/cache/two.png', name: 'Alice Smith.png' },
    ];
    const png = new Uint8Array([137, 80, 78, 71]);
    const { characterCards, discarded } = installHarness({
        files: selected,
        readFile: path => path.endsWith('.png') ? png : new TextEncoder().encode('{"name":"Alice"}'),
    });
    const files = await characterCards.pickFiles({ multiple: true });
    assert.deepEqual(files.map(file => [file.name, file.type]), [
        ['Alice%20.json', 'application/json'], ['Alice Smith.png', 'image/png'],
    ]);
    assert.equal(await files[0].text(), '{"name":"Alice"}');
    assert.deepEqual(new Uint8Array(await files[1].arrayBuffer()), png);
    assert.deepEqual(discarded, selected.map(file => file.path));
});
