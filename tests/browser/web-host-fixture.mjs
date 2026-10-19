export function createWebHostFixture(defaultSettings, theme) {
    const settings = structuredClone(defaultSettings);
    Object.assign(settings, { firstRun: false, currentVersion: '2.3.0', active_character: 'Review.png', active_group: null });
    Object.assign(settings.power_user, { tokenizer: 0, auto_load_chat: true, theme_fallback: 'Dark Lite' });
    settings.power_user.personas = { 'user-default.png': 'User' };
    settings.power_user.persona_descriptions = {
        'user-default.png': { description: 'A saved persona.', position: 0, depth: 2, role: 0, lorebook: '', title: '' },
    };
    const character = {
        name: 'Review', avatar: 'Review.png', chat: 'review-chat',
        description: '', personality: '', scenario: '', first_mes: 'Hello', mes_example: '',
        create_date: '2026-01-01T12:00:00Z', creator: '', creator_notes: '', character_version: '',
        tags: [], talkativeness: 0.5, fav: false, chat_size: 200, data_size: 200,
        date_added: 1, date_last_chat: 1, alternate_greetings: [], system_prompt: '', post_history_instructions: '',
        data: { name: 'Review', extensions: { world: 'Review lore' } },
    };
    const chat = [
        { user_name: 'User', character_name: 'Review', chat_metadata: { integrity: 'review-integrity' } },
        { name: 'Review', is_user: false, is_system: false, mes: 'A chat loaded through Web Host.', send_date: '2026-01-01T12:00:00Z', extra: {} },
    ];
    const lore = { entries: { 0: { uid: 0, key: ['Review'], content: 'Review lore loaded.', disable: false, order: 100, position: 0 } } };
    const payload = {
        settings: JSON.stringify(settings), enable_extensions: true, enable_extensions_auto_update: false, enable_accounts: false,
        koboldai_settings: [], koboldai_setting_names: [], novelai_settings: [], novelai_setting_names: [],
        openai_settings: [], openai_setting_names: [], textgenerationwebui_presets: [], textgenerationwebui_preset_names: [],
        themes: [theme],
        moving_ui_presets: [], quick_reply_presets: [], instruct: [], context: [], sysprompt: [], reasoning: [],
        world_names: ['Review lore'],
    };
    let state = null;
    const respond = (body, status = 200) => ({ status, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
    const backend = {
        handle(request) {
            switch (request.url) {
                case '/version': return respond({ agent: 'SillyTavern:1.19.0:TauriTavern', pkgVersion: '2.3.0', tauriVersion: '2.3.0', defaultUpdateChannel: 'stable' });
                case '/api/settings/get': return respond(payload);
                case '/api/settings/save': payload.settings = request.body; return respond({ result: 'ok' });
                case '/api/characters/all': return respond([character]);
                case '/api/characters/chats': return respond([{ file_name: 'review-chat.jsonl', chat_items: 1, file_size: '200 B', mes: chat[1].mes, last_mes: chat[1].send_date }]);
                case '/api/groups/all': return respond([]);
                case '/api/avatars/get': return respond([]);
                case '/api/secrets/read': return respond({});
                case '/api/backgrounds/all': return respond({ images: [], config: { width: 160, height: 90 } });
                case '/api/backgrounds/folders': return respond({ folders: [], imageFolderMap: {} });
                case '/api/image-metadata/all': return respond({});
                case '/api/extensions/discover': return respond([]);
                case '/api/chats/get': return respond(chat);
                case '/api/worldinfo/get': return respond(lore);
                default: throw new Error(`Unexpected ST backend request: ${request.method} ${request.url}`);
            }
        },
    };
    return {
        backend,
        store: { load: () => state, save: value => { state = structuredClone(value); } },
    };
}
