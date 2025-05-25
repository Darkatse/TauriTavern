// Tauri Chats API for SillyTavern
// This module provides API functions for managing chats in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a chats API client
const chatsClient = createApiClient('chats');

/**
 * Get all chats
 * @returns {Promise<Array>} - List of chats
 */
export async function getAllChats() {
    try {
        return await chatsClient.call('get-all', {}, {
            path: 'all',
            commandName: 'get_all_chats'
        });
    } catch (error) {
        console.error('Error getting all chats:', error);
        throw error;
    }
}

/**
 * Get a chat by character name and file name
 * @param {string} characterName - Character name
 * @param {string} fileName - File name
 * @returns {Promise<Object>} - Chat data
 */
export async function getChat(characterName, fileName) {
    try {
        return await chatsClient.call('get', { characterName: characterName, fileName: fileName }, {
            path: 'get',
            commandName: 'get_chat'
        });
    } catch (error) {
        console.error(`Error getting chat ${characterName}/${fileName}:`, error);
        throw error;
    }
}

/**
 * Get chats for a character
 * @param {string} characterName - Character name
 * @returns {Promise<Array>} - List of chats
 */
export async function getCharacterChats(characterName) {
    try {
        return await chatsClient.call('get-character-chats', { character_name: characterName }, {
            path: 'character-chats',
            commandName: 'get_character_chats'
        });
    } catch (error) {
        console.error(`Error getting chats for character ${characterName}:`, error);
        throw error;
    }
}

/**
 * Create a new chat
 * @param {Object} chatData - Chat data
 * @returns {Promise<Object>} - Created chat
 */
export async function createChat(chatData) {
    try {
        return await chatsClient.call('create', { dto: chatData }, {
            path: 'create',
            commandName: 'create_chat'
        });
    } catch (error) {
        console.error('Error creating chat:', error);
        throw error;
    }
}

/**
 * Add a message to a chat
 * @param {Object} messageData - Message data
 * @returns {Promise<Object>} - Updated chat
 */
export async function addMessage(messageData) {
    try {
        return await chatsClient.call('add-message', { dto: messageData }, {
            path: 'add-message',
            commandName: 'add_message'
        });
    } catch (error) {
        console.error('Error adding message to chat:', error);
        throw error;
    }
}

/**
 * Rename a chat
 * @param {Object} dto - Rename chat DTO
 * @returns {Promise<void>}
 */
export async function renameChat(dto) {
    try {
        return await chatsClient.call('rename', { dto }, {
            path: 'rename',
            commandName: 'rename_chat'
        });
    } catch (error) {
        console.error(`Error renaming chat ${dto.character_name}/${dto.old_file_name} to ${dto.new_file_name}:`, error);
        throw error;
    }
}

/**
 * Delete a chat
 * @param {string} characterName - Character name
 * @param {string} fileName - File name
 * @returns {Promise<void>}
 */
export async function deleteChat(characterName, fileName) {
    try {
        return await chatsClient.call('delete', { characterName: characterName, fileName: fileName }, {
            path: 'delete',
            commandName: 'delete_chat'
        });
    } catch (error) {
        console.error(`Error deleting chat ${characterName}/${fileName}:`, error);
        throw error;
    }
}

/**
 * Search for chats
 * @param {string} query - Search query
 * @param {string} characterFilter - Character filter (optional)
 * @returns {Promise<Array>} - List of chat search results
 */
export async function searchChats(query, characterFilter = null) {
    try {
        return await chatsClient.call('search', { query, characterFilter: characterFilter }, {
            path: 'search',
            commandName: 'search_chats'
        });
    } catch (error) {
        console.error(`Error searching chats with query "${query}":`, error);
        throw error;
    }
}

/**
 * Import a chat
 * @param {Object} dto - Import chat DTO
 * @returns {Promise<Object>} - Imported chat
 */
export async function importChat(dto) {
    try {
        return await chatsClient.call('import', { dto }, {
            path: 'import',
            commandName: 'import_chat'
        });
    } catch (error) {
        console.error('Error importing chat:', error);
        throw error;
    }
}

/**
 * Export a chat
 * @param {Object} dto - Export chat DTO
 * @returns {Promise<void>}
 */
export async function exportChat(dto) {
    try {
        return await chatsClient.call('export', { dto }, {
            path: 'export',
            commandName: 'export_chat'
        });
    } catch (error) {
        console.error(`Error exporting chat ${dto.characterName}/${dto.fileName}:`, error);
        throw error;
    }
}

/**
 * Backup a chat
 * @param {string} characterName - Character name
 * @param {string} fileName - File name
 * @returns {Promise<void>}
 */
export async function backupChat(characterName, fileName) {
    try {
        return await chatsClient.call('backup', { characterName: characterName, fileName: fileName }, {
            path: 'backup',
            commandName: 'backup_chat'
        });
    } catch (error) {
        console.error(`Error backing up chat ${characterName}/${fileName}:`, error);
        throw error;
    }
}

/**
 * Clear chat cache
 * @returns {Promise<void>}
 */
export async function clearChatCache() {
    try {
        return await chatsClient.call('clear-cache', {}, {
            path: 'clear-cache',
            commandName: 'clear_chat_cache'
        });
    } catch (error) {
        console.error('Error clearing chat cache:', error);
        throw error;
    }
}
