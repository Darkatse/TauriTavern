// Tauri API Module for SillyTavern
// This module provides API functions for communicating with the Tauri backend

import * as TauriBridge from './tauri-bridge.js';
import * as CharactersAPI from './scripts/tauri/characters-api.js';
import * as ChatsAPI from './scripts/tauri/chats-api.js';
import * as SettingsAPI from './scripts/tauri/settings-api.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

// Re-export the API functions with error handling
// These functions are deprecated and will be removed in a future version
// Use the adapter pattern instead (see scripts/tauri/*.js)

// Character API
export async function getCharacters(shallow = true) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use characters.getCharacters() instead of tauri-api.getCharacters()');
        return await CharactersAPI.getAllCharacters(shallow);
    } catch (error) {
        console.error('Error getting characters from Tauri backend:', error);
        return null;
    }
}

export async function getCharacter(name) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use characters.getCharacter() instead of tauri-api.getCharacter()');
        return await CharactersAPI.getCharacter(name);
    } catch (error) {
        console.error(`Error getting character ${name} from Tauri backend:`, error);
        return null;
    }
}

export async function createCharacter(characterData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use characters.createCharacter() instead of tauri-api.createCharacter()');
        return await CharactersAPI.createCharacter(characterData);
    } catch (error) {
        console.error('Error creating character in Tauri backend:', error);
        return null;
    }
}

export async function updateCharacter(name, characterData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use characters.updateCharacter() instead of tauri-api.updateCharacter()');
        return await CharactersAPI.updateCharacter(name, characterData);
    } catch (error) {
        console.error('Error updating character in Tauri backend:', error);
        return null;
    }
}

export async function deleteCharacter(dto) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use characters.deleteCharacter() instead of tauri-api.deleteCharacter()');
        await CharactersAPI.deleteCharacter(dto);
        return true;
    } catch (error) {
        console.error(`Error deleting character ${dto.name} from Tauri backend:`, error);
        return null;
    }
}

// Chat API
export async function getChats() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.getAllChats() instead of tauri-api.getChats()');
        return await ChatsAPI.getAllChats();
    } catch (error) {
        console.error('Error getting chats from Tauri backend:', error);
        return null;
    }
}

export async function getChat(characterName, fileName) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.getChat() instead of tauri-api.getChat()');
        return await ChatsAPI.getChat(characterName, fileName);
    } catch (error) {
        console.error(`Error getting chat ${characterName}/${fileName} from Tauri backend:`, error);
        return null;
    }
}

export async function getChatsByCharacter(characterName) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.getCharacterChats() instead of tauri-api.getChatsByCharacter()');
        return await ChatsAPI.getCharacterChats(characterName);
    } catch (error) {
        console.error(`Error getting chats for character ${characterName} from Tauri backend:`, error);
        return null;
    }
}

export async function createChat(chatData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.createChat() instead of tauri-api.createChat()');
        return await ChatsAPI.createChat(chatData);
    } catch (error) {
        console.error('Error creating chat in Tauri backend:', error);
        return null;
    }
}

export async function addMessage(messageData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.addMessage() instead of tauri-api.addMessage()');
        return await ChatsAPI.addMessage(messageData);
    } catch (error) {
        console.error('Error adding message in Tauri backend:', error);
        return null;
    }
}

export async function deleteChat(characterName, fileName) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use chat.deleteChat() instead of tauri-api.deleteChat()');
        await ChatsAPI.deleteChat(characterName, fileName);
        return true;
    } catch (error) {
        console.error(`Error deleting chat ${characterName}/${fileName} from Tauri backend:`, error);
        return null;
    }
}

// Settings API
export async function getSettings() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use settings.getSettings() instead of tauri-api.getSettings()');
        return await SettingsAPI.getSettings();
    } catch (error) {
        console.error('Error getting settings from Tauri backend:', error);
        return null;
    }
}

export async function updateSettings(settingsData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use settings.updateSettings() instead of tauri-api.updateSettings()');
        return await SettingsAPI.updateSettings(settingsData);
    } catch (error) {
        console.error('Error updating settings in Tauri backend:', error);
        return null;
    }
}

// Utility functions
export async function getVersion() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use TauriBridge.getVersion() directly');
        return await TauriBridge.getVersion();
    } catch (error) {
        console.error('Error getting version from Tauri backend:', error);
        return null;
    }
}

/**
 * Get detailed client version information
 * @returns {Promise<Object>} Version information object
 */
export async function getClientVersion() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        console.warn('Deprecated: Use TauriBridge.getClientVersion() directly');
        console.log('Getting client version from Tauri API');
        const versionInfo = await TauriBridge.getClientVersion();
        console.log('Client version from Tauri API:', versionInfo);
        return versionInfo;
    } catch (error) {
        console.error('Error getting client version from Tauri API:', error);
        return null;
    }
}

// Export a function to check if we're running in Tauri
export function isTauriEnvironment() {
    console.warn('Deprecated: Use TauriBridge.isTauri() instead');
    return isTauri;
}
