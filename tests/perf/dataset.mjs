// The data the perf suite runs on, served to the page through the Web Host.
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

