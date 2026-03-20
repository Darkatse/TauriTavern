// @ts-check

import { getTauriTavernSettings } from '../../../../tauri-bridge.js';
import {
    normalizeChatHistoryModeName,
    writeStoredChatHistoryModeName,
} from './chat-history-mode-state.js';

export function installChatHistoryMode() {
    const ready = getTauriTavernSettings().then((settings) => {
        const modeName = normalizeChatHistoryModeName(settings.chat_history_mode);
        writeStoredChatHistoryModeName(modeName);
        return modeName;
    });

    return { ready };
}
