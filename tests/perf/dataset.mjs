// The data the perf suites run on. The frontend suite serves it through the Web Host; the backend replay reads the
// same data from a data root laid out like the native host's.
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { createWebHostFixture } from '../browser/web-host-fixture.mjs';
import { defaultSettings, defaultTheme, systemExtensions } from '../helpers/frontend-content.mjs';

/**
 * A long chat and a large lorebook; the first entry holds 100k characters. Ledger is a heavy card: an embedded
 * lorebook of about 2M characters, and a chat whose last reply holds about 150k characters.
 */
export function dataset() {
    const paragraph = 'The old lighthouse keeper remembers every ship that passed the northern reef, and speaks of them as friends. ';
    const chat = [{ user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } }];
    for (let i = 0; i < 80; i++) {
        chat.push({
            name: i % 2 ? 'User' : 'Review', is_user: i % 2 === 1, is_system: false, send_date: '2026-01-01T12:00:00Z',
            // Replies carry their model icon, as in a real chat, so a new reply's icon is already loaded.
            extra: i % 2 ? {} : { api: 'custom', model: 'replay' },
            mes: `*She glances at the map.* "${paragraph.repeat(3)}"\n\n**Note:** ${paragraph}`,
        });
    }
    const entries = {};
    for (let uid = 0; uid < 200; uid++) {
        entries[uid] = {
            uid, key: [`key${uid}`, `alias${uid}`], keysecondary: [], comment: `Entry ${uid}`, content: paragraph.repeat(8),
            constant: false, selective: true, order: 100, position: 0, disable: false, displayIndex: uid, probability: 100, useProbability: true,
        };
    }
    entries[0].content = `${paragraph.repeat(4)}\n\n`.repeat(250).slice(0, 100_000);
    const book = {
        entries: Array.from({ length: 1000 }, (_, id) => ({
            id, keys: [`ledger${id}`], content: paragraph.repeat(18), enabled: true, insertion_order: 100, extensions: {},
        })),
    };
    const ledgerCard = JSON.stringify({
        spec: 'chara_card_v2', spec_version: '2.0', name: 'Ledger', chat: 'ledger-chat', create_date: '2026-01-02T12:00:00Z',
        data: {
            name: 'Ledger', description: paragraph.repeat(20), personality: '', scenario: '', first_mes: paragraph.repeat(10),
            mes_example: '', creator: '', creator_notes: '', character_version: '', tags: [], alternate_greetings: [],
            system_prompt: '', post_history_instructions: '', character_book: book, extensions: { talkativeness: 0.5, fav: false },
        },
    });
    const ledgerChat = [{ user_name: 'User', character_name: 'Ledger', chat_metadata: { integrity: 'ledger-integrity' } }];
    for (let i = 0; i < 6; i++) {
        ledgerChat.push({
            name: i % 2 ? 'User' : 'Ledger', is_user: i % 2 === 1, is_system: false, send_date: `2026-01-02T12:0${i}:00Z`,
            extra: i % 2 ? {} : { api: 'custom', model: 'replay' },
            mes: i === 5 ? paragraph.repeat(1400) : paragraph.repeat(4),
        });
    }
    return { chat, lore: { entries }, ledgerCard, ledgerChat };
}

export function perfSettings() {
    const settings = structuredClone(defaultSettings);
    // With the browser flags in openFrontend, no animation spreads a step's work over time.
    settings.power_user.reduced_motion = true;
    // The embedded-lorebook import prompt shows once per card, and this user has already answered it for Ledger.
    settings.accountStorage = { ...settings.accountStorage, 'AlertWI_Ledger.png': 'true' };
    return settings;
}

/** The page's Web Host data: the fixture's arguments plus the dataset. */
export function pageData() {
    return { settings: perfSettings(), theme: defaultTheme, extensions: systemExtensions, ...dataset() };
}

const jsonl = records => records.map(record => JSON.stringify(record)).join('\n');

/** A 1x1 PNG carrying a character card in its `chara` text chunk, as SillyTavern writes cards. */
function cardPng(cardJson) {
    const chunk = (type, data) => {
        const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
        const length = Buffer.alloc(4), checksum = Buffer.alloc(4);
        length.writeUInt32BE(data.length);
        checksum.writeUInt32BE(crc32(body));
        return Buffer.concat([length, body, checksum]);
    };
    const header = Buffer.from([0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk('IHDR', header),
        chunk('tEXt', Buffer.from(`chara\0${Buffer.from(cardJson).toString('base64')}`, 'latin1')),
        chunk('IDAT', deflateSync(Buffer.from([0, 0, 0, 0, 0]))),
        chunk('IEND', Buffer.alloc(0)),
    ]);
}

/** Writes the dataset under `root` in the native host's data directory layout. */
export async function writeDataRoot(root) {
    const data = pageData();
    const fixture = createWebHostFixture(data.settings, data.theme, data.extensions);
    const get = url => JSON.parse(fixture.backend.handle({ method: 'POST', url, headers: {}, body: '{}' }).body);
    const user = path.join(root, 'default-user');
    const files = {
        'settings.json': get('/api/settings/get').settings,
        'characters/Review.png': cardPng(get('/api/characters/get').json_data),
        'characters/Ledger.png': cardPng(data.ledgerCard),
        'chats/Review/review-chat.jsonl': jsonl(data.chat),
        'chats/Ledger/ledger-chat.jsonl': jsonl(data.ledgerChat),
        'worlds/Review lore.json': JSON.stringify(data.lore),
    };
    for (const [file, content] of Object.entries(files)) {
        await mkdir(path.dirname(path.join(user, file)), { recursive: true });
        await writeFile(path.join(user, file), content);
    }
}
