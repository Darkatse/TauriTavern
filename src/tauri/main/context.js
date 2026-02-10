export function createTauriMainContext({ invoke, convertFileSrc }) {
    let userDirectories = null;
    let characterCache = [];
    let characterByAvatar = new Map();
    let characterByName = new Map();

    async function initialize() {
        await loadUserDirectories();
        installAssetPathHelpers();
    }

    async function loadUserDirectories() {
        let lastError = null;

        for (let attempt = 0; attempt < 40; attempt += 1) {
            try {
                userDirectories = await invoke('get_default_user_directory');
                return;
            } catch (error) {
                lastError = error;
                await sleep(250);
            }
        }

        console.warn('Could not load default user directories:', lastError);
    }

    function installAssetPathHelpers() {
        window.__TAURITAVERN_THUMBNAIL__ = (type, file, useTimestamp = false) => {
            const filePath = resolveAssetPath(type, file);

            if (filePath) {
                const assetUrl = toAssetUrl(filePath);
                if (assetUrl) {
                    return `${assetUrl}${useTimestamp ? `?t=${Date.now()}` : ''}`;
                }
            }

            return `/thumbnail?type=${encodeURIComponent(type)}&file=${encodeURIComponent(file)}${useTimestamp ? `&t=${Date.now()}` : ''}`;
        };

        window.__TAURITAVERN_BACKGROUND_PATH__ = (file) => {
            const filePath = resolveAssetPath('bg', file);
            const assetUrl = filePath ? toAssetUrl(filePath) : null;
            return assetUrl || `backgrounds/${encodeURIComponent(file)}`;
        };

        window.__TAURITAVERN_PERSONA_PATH__ = (file) => {
            const filePath = resolveAssetPath('persona', file);
            const assetUrl = filePath ? toAssetUrl(filePath) : null;
            return assetUrl || `User Avatars/${file}`;
        };
    }

    function resolveAssetPath(type, file) {
        if (!userDirectories || !file) {
            return null;
        }

        const cleanFile = sanitizeRelativePath(file);
        if (!cleanFile) {
            return null;
        }

        switch (type) {
            case 'avatar':
                return joinPath(userDirectories.characters, cleanFile);
            case 'persona':
                return joinPath(userDirectories.avatars, cleanFile);
            case 'bg':
                return joinPath(userDirectories.backgrounds, cleanFile);
            default:
                return null;
        }
    }

    function toAssetUrl(path) {
        if (!path) {
            return null;
        }

        if (typeof convertFileSrc === 'function') {
            try {
                return convertFileSrc(path);
            } catch (error) {
                console.warn('convertFileSrc failed:', error);
            }
        }

        return null;
    }

    function joinPath(base, child) {
        if (!base) {
            return null;
        }

        const normalizedBase = String(base).replace(/[\\/]+$/, '');
        const normalizedChild = String(child).replace(/^[\\/]+/, '');
        const separator = normalizedBase.includes('\\') ? '\\' : '/';

        return `${normalizedBase}${separator}${normalizedChild.replace(/[\\/]+/g, separator)}`;
    }

    function sanitizeRelativePath(file) {
        const decoded = decodeURIComponent(String(file));
        const normalized = decoded.replace(/[\\/]+/g, '/').replace(/^\/+/, '');

        if (!normalized || normalized.includes('..')) {
            return null;
        }

        return normalized;
    }

    async function safeInvoke(command, args = {}) {
        for (let attempt = 0; attempt < 20; attempt += 1) {
            try {
                return await invoke(command, args);
            } catch (error) {
                const message = error?.message || error?.toString?.() || `Command failed: ${command}`;
                if (attempt < 19 && shouldRetryInvoke(message)) {
                    await sleep(200);
                    continue;
                }
                throw new Error(message);
            }
        }
    }

    function shouldRetryInvoke(message) {
        const normalized = String(message || '').toLowerCase();
        return (
            (normalized.includes('state') && normalized.includes('not managed')) ||
            normalized.includes('invoke is unavailable')
        );
    }

    function normalizeCharacter(character) {
        if (!character || typeof character !== 'object') {
            return character;
        }

        const extensions = normalizeExtensions(character.extensions);

        if (!Object.prototype.hasOwnProperty.call(extensions, 'talkativeness')) {
            extensions.talkativeness = Number(character.talkativeness ?? 0.5);
        }

        if (!Object.prototype.hasOwnProperty.call(extensions, 'fav')) {
            extensions.fav = Boolean(character.fav);
        }

        const data = {
            name: character.name,
            description: character.description || '',
            personality: character.personality || '',
            scenario: character.scenario || '',
            first_mes: character.first_mes || '',
            mes_example: character.mes_example || '',
            creator: character.creator || '',
            creator_notes: character.creator_notes || '',
            character_version: character.character_version || '',
            system_prompt: character.system_prompt || '',
            post_history_instructions: character.post_history_instructions || '',
            tags: Array.isArray(character.tags) ? character.tags : [],
            alternate_greetings: Array.isArray(character.alternate_greetings) ? character.alternate_greetings : [],
            extensions,
        };

        return {
            ...character,
            creatorcomment: character.creator_notes || '',
            data,
            shallow: false,
        };
    }

    function normalizeExtensions(input) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            return {};
        }

        return { ...input };
    }

    function updateCharacterCache(characters) {
        characterCache = Array.isArray(characters) ? characters : [];
        characterByAvatar = new Map();
        characterByName = new Map();

        for (const character of characterCache) {
            if (character?.avatar) {
                characterByAvatar.set(String(character.avatar), character);
            }

            if (character?.name) {
                characterByName.set(String(character.name), character);
            }
        }
    }

    async function getAllCharacters({ shallow = false, forceRefresh = false } = {}) {
        if (!forceRefresh && characterCache.length > 0) {
            return characterCache;
        }

        const characters = await safeInvoke('get_all_characters', { shallow });
        const normalized = Array.isArray(characters) ? characters.map(normalizeCharacter) : [];
        updateCharacterCache(normalized);
        return normalized;
    }

    async function resolveCharacterName({ avatar, fallbackName } = {}) {
        if (fallbackName) {
            return String(fallbackName);
        }

        if (!avatar) {
            return null;
        }

        const avatarKey = String(avatar);
        const cached = characterByAvatar.get(avatarKey);
        if (cached?.name) {
            return cached.name;
        }

        await getAllCharacters({ shallow: false });
        const refreshed = characterByAvatar.get(avatarKey);
        return refreshed?.name || null;
    }

    async function getSingleCharacter(body) {
        const explicitName = body?.name || body?.ch_name;
        const avatar = body?.avatar_url || body?.avatar;
        const characterName = await resolveCharacterName({ avatar, fallbackName: explicitName });

        if (!characterName) {
            return null;
        }

        const character = await safeInvoke('get_character', { name: characterName });
        const normalized = normalizeCharacter(character);
        if (normalized?.avatar) {
            characterByAvatar.set(String(normalized.avatar), normalized);
        }
        if (normalized?.name) {
            characterByName.set(String(normalized.name), normalized);
        }
        return normalized;
    }

    function ensureJsonl(fileName) {
        const value = String(fileName || '');
        if (!value) {
            return value;
        }

        return value.endsWith('.jsonl') ? value : `${value}.jsonl`;
    }

    function stripJsonl(fileName) {
        const value = String(fileName || '').trim();
        if (!value) {
            return '';
        }

        return value.replace(/\.jsonl$/i, '');
    }

    function toFrontendChat(chatDto) {
        const messages = Array.isArray(chatDto?.messages)
            ? chatDto.messages.map((message) => ({
                name: message.name,
                is_user: Boolean(message.is_user),
                is_system: Boolean(message.is_system),
                send_date: message.send_date,
                mes: message.mes,
                extra: message.extra || {},
            }))
            : [];

        const header = {
            user_name: chatDto?.user_name || 'User',
            character_name: chatDto?.character_name || '',
            create_date: chatDto?.create_date || '',
            chat_metadata: {
                chat_id_hash: Number(chatDto?.chat_id || 0),
            },
        };

        return [header, ...messages];
    }

    function formatFileSize(value) {
        const size = Number(value || 0);
        if (!Number.isFinite(size) || size <= 0) {
            return '0 B';
        }

        const units = ['B', 'KB', 'MB', 'GB'];
        let unitIndex = 0;
        let result = size;

        while (result >= 1024 && unitIndex < units.length - 1) {
            result /= 1024;
            unitIndex += 1;
        }

        return `${result.toFixed(result >= 10 ? 0 : 1)} ${units[unitIndex]}`;
    }

    function parseTimestamp(sendDate) {
        const parsed = Date.parse(String(sendDate || ''));
        return Number.isFinite(parsed) ? parsed : Date.now();
    }

    function exportChatAsText(frontendChat) {
        const lines = frontendChat.slice(1).map((message) => {
            const role = message.is_system ? 'System' : message.name || (message.is_user ? 'User' : 'Assistant');
            return `${role}: ${message.mes || ''}`;
        });

        return lines.join('\n\n');
    }

    function exportChatAsJsonl(frontendChat) {
        return frontendChat.map((item) => JSON.stringify(item)).join('\n');
    }

    function findAvatarByCharacterName(name) {
        const character = characterByName.get(String(name || ''));
        return character?.avatar || '';
    }

    async function uniqueCharacterName(baseName) {
        await getAllCharacters({ shallow: false });

        if (!characterByName.has(baseName)) {
            return baseName;
        }

        let index = 2;
        while (characterByName.has(`${baseName} ${index}`)) {
            index += 1;
        }

        return `${baseName} ${index}`;
    }

    function boolFromForm(formData, key) {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return false;
        }

        const value = String(raw).trim().toLowerCase();
        return value === 'true' || value === '1' || value === 'on' || value === 'yes';
    }

    function numberFromForm(formData, key, fallback) {
        const raw = formData.get(key);
        const value = Number(raw);
        return Number.isFinite(value) ? value : fallback;
    }

    function stringFromForm(formData, key, fallback = '') {
        const raw = formData.get(key);
        if (raw === null || raw === undefined) {
            return fallback;
        }

        return String(raw);
    }

    function splitTags(tagsRaw) {
        if (Array.isArray(tagsRaw)) {
            return tagsRaw.map((tag) => String(tag).trim()).filter(Boolean);
        }

        if (typeof tagsRaw === 'string') {
            return tagsRaw.split(',').map((tag) => tag.trim()).filter(Boolean);
        }

        return [];
    }

    function parseJsonSafe(value, fallback = {}) {
        if (typeof value !== 'string' || !value.trim()) {
            return fallback;
        }

        try {
            return JSON.parse(value);
        } catch {
            return fallback;
        }
    }

    function buildCharacterExtensions(formData) {
        const extensions = parseJsonSafe(stringFromForm(formData, 'extensions', ''), {});

        const world = stringFromForm(formData, 'world', '').trim();
        if (world) {
            extensions.world = world;
        }

        const depthPrompt = stringFromForm(formData, 'depth_prompt_prompt', '').trim();
        if (depthPrompt) {
            extensions.depth_prompt = {
                prompt: depthPrompt,
                depth: numberFromForm(formData, 'depth_prompt_depth', 4),
                role: stringFromForm(formData, 'depth_prompt_role', 'system'),
            };
        }

        extensions.talkativeness = numberFromForm(formData, 'talkativeness', 0.5);
        extensions.fav = boolFromForm(formData, 'fav');

        return extensions;
    }

    function formDataToCreateCharacterDto(formData) {
        return {
            name: stringFromForm(formData, 'ch_name', '').trim(),
            description: stringFromForm(formData, 'description', ''),
            personality: stringFromForm(formData, 'personality', ''),
            scenario: stringFromForm(formData, 'scenario', ''),
            first_mes: stringFromForm(formData, 'first_mes', ''),
            mes_example: stringFromForm(formData, 'mes_example', ''),
            creator: stringFromForm(formData, 'creator', ''),
            creator_notes: stringFromForm(formData, 'creator_notes', ''),
            character_version: stringFromForm(formData, 'character_version', ''),
            tags: splitTags(stringFromForm(formData, 'tags', '')),
            talkativeness: numberFromForm(formData, 'talkativeness', 0.5),
            fav: boolFromForm(formData, 'fav'),
            alternate_greetings: formData.getAll('alternate_greetings').map((item) => String(item)).filter(Boolean),
            system_prompt: stringFromForm(formData, 'system_prompt', ''),
            post_history_instructions: stringFromForm(formData, 'post_history_instructions', ''),
            extensions: buildCharacterExtensions(formData),
        };
    }

    function formDataToUpdateCharacterDto(formData) {
        const dto = formDataToCreateCharacterDto(formData);
        return {
            name: dto.name,
            description: dto.description,
            personality: dto.personality,
            scenario: dto.scenario,
            first_mes: dto.first_mes,
            mes_example: dto.mes_example,
            creator: dto.creator,
            creator_notes: dto.creator_notes,
            character_version: dto.character_version,
            tags: dto.tags,
            talkativeness: dto.talkativeness,
            fav: dto.fav,
            alternate_greetings: dto.alternate_greetings,
            system_prompt: dto.system_prompt,
            post_history_instructions: dto.post_history_instructions,
            extensions: dto.extensions,
        };
    }

    async function createCharacterFromForm(formData, requestUrl) {
        const dto = formDataToCreateCharacterDto(formData);
        const crop = parseCropParam(requestUrl);
        const file = formData.get('avatar');

        if (file instanceof File && file.size > 0) {
            const fileInfo = await materializeUploadFile(file);
            if (fileInfo?.filePath) {
                try {
                    return await safeInvoke('create_character_with_avatar', {
                        dto: {
                            character: dto,
                            avatar_path: fileInfo.filePath,
                            crop,
                        },
                    });
                } finally {
                    await fileInfo.cleanup?.();
                }
            }
        }

        return safeInvoke('create_character', { dto });
    }

    async function editCharacterFromForm(formData, requestUrl) {
        const avatar = stringFromForm(formData, 'avatar_url', '');
        const fallbackName = stringFromForm(formData, 'ch_name', '');
        const originalName = await resolveCharacterName({ avatar, fallbackName });

        if (!originalName) {
            throw new Error('Character not found for edit');
        }

        const dto = formDataToUpdateCharacterDto(formData);
        await safeInvoke('update_character', { name: originalName, dto });

        const file = formData.get('avatar');
        if (file instanceof File && file.size > 0) {
            const crop = parseCropParam(requestUrl);
            const fileInfo = await materializeUploadFile(file);

            if (fileInfo?.filePath) {
                try {
                    await safeInvoke('update_avatar', {
                        dto: {
                            name: dto.name || originalName,
                            avatar_path: fileInfo.filePath,
                            crop,
                        },
                    });
                } finally {
                    await fileInfo.cleanup?.();
                }
            }
        }
    }

    function parseCropParam(url) {
        const raw = url.searchParams.get('crop');
        if (!raw) {
            return null;
        }

        try {
            return JSON.parse(raw);
        } catch {
            return null;
        }
    }

    function pickCharacterUpdateFields(payload) {
        const dto = {};
        const keys = [
            'name',
            'description',
            'personality',
            'scenario',
            'first_mes',
            'mes_example',
            'creator',
            'creator_notes',
            'character_version',
            'tags',
            'talkativeness',
            'fav',
            'alternate_greetings',
            'system_prompt',
            'post_history_instructions',
            'extensions',
        ];

        for (const key of keys) {
            if (Object.prototype.hasOwnProperty.call(payload, key)) {
                dto[key] = payload[key];
            }
        }

        return dto;
    }

    async function uploadAvatarFromForm(formData, requestUrl) {
        const file = formData.get('avatar');
        if (!(file instanceof File)) {
            throw new Error('No avatar file provided');
        }

        const overwriteNameRaw = formData.get('overwrite_name');
        const overwriteName = overwriteNameRaw ? String(overwriteNameRaw) : null;
        const crop = parseCropParam(requestUrl);

        const fileInfo = await materializeUploadFile(file);
        if (!fileInfo?.filePath) {
            throw new Error('Unable to access avatar file path');
        }

        try {
            return await safeInvoke('upload_avatar', {
                file_path: fileInfo.filePath,
                overwrite_name: overwriteName,
                crop: crop ? JSON.stringify(crop) : null,
            });
        } finally {
            await fileInfo.cleanup?.();
        }
    }

    async function materializeUploadFile(file) {
        const directPath = extractNativeFilePath(file);
        if (directPath) {
            return { filePath: directPath };
        }

        const tauri = window.__TAURI__;
        const hasLegacyFs = Boolean(tauri?.path?.tempDir && tauri?.path?.join && tauri?.fs?.writeBinaryFile);

        if (!hasLegacyFs) {
            return null;
        }

        const tempDir = await tauri.path.tempDir();
        const extension = (file.name && file.name.includes('.')) ? `.${file.name.split('.').pop()}` : '.bin';
        const fileName = `tauritavern-upload-${Date.now()}-${Math.random().toString(16).slice(2)}${extension}`;
        const filePath = await tauri.path.join(tempDir, fileName);
        const data = new Uint8Array(await file.arrayBuffer());

        await tauri.fs.writeBinaryFile(filePath, data);

        return {
            filePath,
            cleanup: async () => {
                try {
                    await tauri.fs.removeFile(filePath);
                } catch {
                    // noop
                }
            },
        };
    }

    function extractNativeFilePath(file) {
        if (!file || typeof file !== 'object') {
            return null;
        }

        const candidate = file.path || file.webkitRelativePath || null;
        if (!candidate || typeof candidate !== 'string') {
            return null;
        }

        if (candidate.toLowerCase().includes('fakepath')) {
            return null;
        }

        return candidate;
    }

    function sleep(ms) {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }

    return {
        initialize,
        safeInvoke,
        normalizeCharacter,
        normalizeExtensions,
        getAllCharacters,
        resolveCharacterName,
        getSingleCharacter,
        ensureJsonl,
        stripJsonl,
        toFrontendChat,
        formatFileSize,
        parseTimestamp,
        exportChatAsText,
        exportChatAsJsonl,
        findAvatarByCharacterName,
        uniqueCharacterName,
        pickCharacterUpdateFields,
        createCharacterFromForm,
        editCharacterFromForm,
        uploadAvatarFromForm,
        materializeUploadFile,
    };
}
