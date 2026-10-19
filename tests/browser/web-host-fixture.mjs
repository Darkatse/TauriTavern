export function createWebHostFixture(defaultSettings, theme, extensions) {
    const settings = structuredClone(defaultSettings);
    Object.assign(settings, { firstRun: false, currentVersion: '2.3.0', active_character: 'Review.png', active_group: null });
    Object.assign(settings.power_user, { tokenizer: 0, auto_load_chat: true, theme_fallback: 'Dark Lite' });
    Object.assign(settings.oai_settings, { chat_completion_source: 'custom', custom_url: 'https://replay.invalid/v1', custom_model: 'replay' });
    settings.power_user.personas = { 'user-default.png': 'User' };
    settings.power_user.persona_descriptions = {
        'user-default.png': { description: 'A saved persona.', position: 0, depth: 2, role: 0, lorebook: '', title: '' },
    };
    let cardJson = JSON.stringify({
        spec: 'chara_card_v2', spec_version: '2.0',
        name: 'Review', chat: 'review-chat',
        description: '', personality: '', scenario: '', first_mes: 'Hello', mes_example: '',
        create_date: '2026-01-01T12:00:00Z', creator: '', creator_notes: '', character_version: '',
        tags: [], talkativeness: 0.5, fav: false,
        alternate_greetings: [], system_prompt: '', post_history_instructions: '',
        data: {
            name: 'Review', description: '', personality: '', scenario: '', first_mes: 'Hello', mes_example: '',
            creator: '', creator_notes: '', character_version: '', tags: [], alternate_greetings: [],
            system_prompt: '', post_history_instructions: '',
            extensions: { world: 'Review lore', talkativeness: 0.5, fav: false },
        },
    }, null, 4);
    const character = () => ({ ...JSON.parse(cardJson), json_data: cardJson, avatar: 'Review.png',
        chat_size: 200, data_size: 200, date_added: 1, date_last_chat: 1 });
    const chat = [
        { user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } },
        { name: 'Review', is_user: false, is_system: false, mes: 'A chat loaded through Web Host.', send_date: '2026-01-01T12:00:00Z', extra: {} },
    ];
    const chatFiles = new Map([['review-chat', chat.map(record => JSON.stringify(record)).join('\n')]]);
    // ST drops unparseable records on read, but checks the raw first line before saving.
    const parse = text => { try { return JSON.parse(text); } catch { return null; } };
    const records = text => text.split('\n').map(parse).filter(Boolean);
    const lore = { entries: { 0: { uid: 0, key: ['Review'], content: 'Review lore loaded.', disable: false, order: 100, position: 0 } } };
    const payload = {
        settings: JSON.stringify(settings), enable_extensions: true, enable_extensions_auto_update: false, enable_accounts: false,
        koboldai_settings: [], koboldai_setting_names: [], novelai_settings: [], novelai_setting_names: [],
        openai_settings: [], openai_setting_names: [], textgenerationwebui_presets: [], textgenerationwebui_preset_names: [],
        themes: [theme],
        movingUIPresets: [],
        quickReplyPresets: [{ version: 2, name: 'Review QR', qrList: [{ id: 1, label: 'Continue', message: 'Continue the story' }] }],
        instruct: [], context: [], sysprompt: [], reasoning: [],
        world_names: ['Review lore'],
    };
    let state = null;
    const respond = (body, status = 200) => ({ status, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
    const backend = {
        handle(request) {
            const body = typeof request.body === 'string' ? JSON.parse(request.body) : request.body;
            switch (`${request.method} ${request.url}`) {
                case 'GET /version': return respond({ agent: 'SillyTavern:1.19.0:TauriTavern', pkgVersion: '2.3.0', tauriVersion: '2.3.0', defaultUpdateChannel: 'stable' });
                case 'POST /api/settings/get': return respond(payload);
                case 'POST /api/settings/save': payload.settings = request.body; return respond({ result: 'ok' });
                case 'POST /api/characters/all': return respond([character()]);
                case 'POST /api/characters/get': return respond(character());
                case 'POST /api/characters/edit': {
                    const fields = Object.fromEntries(body.entries);
                    cardJson = fields.json_data;
                    return { status: 200, headers: {}, body: 'OK' };
                }
                case 'POST /api/characters/merge-attributes': {
                    const update = { ...body };
                    delete update.json_data;
                    cardJson = JSON.stringify(merge(JSON.parse(cardJson), update));
                    return respond({ ok: true });
                }
                case 'POST /api/characters/chats': return respond([...chatFiles].map(([file, text]) => {
                    const chat = records(text);
                    return { file_name: `${file}.jsonl`, chat_items: chat.length - 1, file_size: '200 B',
                        mes: chat.at(-1)?.mes, last_mes: chat.at(-1)?.send_date };
                }));
                case 'POST /api/groups/all': return respond([]);
                case 'POST /api/avatars/get': return respond([]);
                case 'POST /api/secrets/read': return respond({});
                case 'POST /api/backgrounds/all': return respond({ images: [], config: { width: 160, height: 90 } });
                case 'POST /api/backgrounds/folders': return respond({ folders: [], imageFolderMap: {} });
                case 'POST /api/image-metadata/all': return respond({});
                case 'GET /api/extensions/discover': return respond(extensions);
                case 'POST /api/quick-replies/save':
                    payload.quickReplyPresets = [...payload.quickReplyPresets.filter(set => set.name !== body.name), body];
                    return { status: 200, headers: {}, body: '' };
                case 'POST /api/chats/get': return respond(chatFiles.has(body.file_name) ? records(chatFiles.get(body.file_name)) : {});
                case 'POST /api/chats/save': {
                    const current = chatFiles.get(body.file_name);
                    const incoming = body.chat[0]?.chat_metadata?.integrity;
                    if (!body.force && incoming && current) {
                        const header = parse(current.split('\n')[0]);
                        if (!header || typeof header !== 'object' || Array.isArray(header)
                            || (header.chat_metadata?.integrity && header.chat_metadata.integrity !== incoming)) {
                            return respond({ error: 'integrity' }, 400);
                        }
                    }
                    chatFiles.set(body.file_name, body.chat.map(record => JSON.stringify(record)).join('\n'));
                    return respond({ ok: true });
                }
                case 'POST /api/themes/save':
                    payload.themes = [...payload.themes.filter(theme => theme.name !== body.name), body];
                    return { status: 200, headers: {}, body: 'OK' };
                case 'POST /api/backends/chat-completions/status': return respond({ data: [{ id: 'replay' }] });
                case 'POST /api/backends/chat-completions/generate': {
                    const reply = 'A replayed reply.';
                    if (!body.stream) return respond({ choices: [{ message: { content: reply } }] });
                    const data = [
                        { choices: [{ delta: { content: 'A replayed ' }, index: 0 }] },
                        { choices: [{ delta: { content: 'reply.' }, index: 0 }] },
                        '[DONE]',
                    ];
                    return {
                        status: 200, headers: { 'content-type': 'text/event-stream' },
                        body: new ReadableStream({ start(controller) {
                            for (const chunk of data) controller.enqueue(new TextEncoder().encode(`data: ${typeof chunk === 'string' ? chunk : JSON.stringify(chunk)}\n\n`));
                            controller.close();
                        } }),
                    };
                }
                case 'POST /api/worldinfo/get': return respond(lore);
                default: throw new Error(`Unexpected ST backend request: ${request.method} ${request.url}`);
            }
        },
    };
    return {
        backend,
        chatFiles,
        store: { load: () => state, save: value => { state = structuredClone(value); } },
    };
}

// The fixture only models JSON card fields; PNG encoding remains the backend's responsibility.
function merge(target, update) {
    const result = { ...target };
    for (const [key, value] of Object.entries(update)) {
        result[key] = value !== null && typeof value === 'object' && !Array.isArray(value)
            ? merge(target?.[key], value) : value;
    }
    return result;
}
