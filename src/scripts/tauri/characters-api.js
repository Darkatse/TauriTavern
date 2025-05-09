// Tauri Characters API for SillyTavern
// This module provides API functions for managing characters in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a characters API client
const charactersClient = createApiClient('characters');

/**
 * Get all characters
 * @param {boolean} shallow - Whether to return shallow character data (without full definitions)
 * @returns {Promise<Array>} - List of characters
 */
export async function getAllCharacters(shallow = true) {
    try {
        return await charactersClient.call('get-all', { shallow }, {
            path: 'all',
            commandName: 'get_all_characters'
        });
    } catch (error) {
        console.error('Error getting all characters:', error);
        throw error;
    }
}

/**
 * Get a character by name
 * @param {string} name - Character name
 * @returns {Promise<Object>} - Character data
 */
export async function getCharacter(name) {
    try {
        return await charactersClient.call('get', { name }, {
            path: 'get',
            commandName: 'get_character'
        });
    } catch (error) {
        console.error(`Error getting character ${name}:`, error);
        throw error;
    }
}

/**
 * Create a new character
 * @param {Object} characterData - Character data
 * @returns {Promise<Object>} - Created character
 */
export async function createCharacter(characterData) {
    try {
        return await charactersClient.call('create', { dto: characterData }, {
            path: 'create',
            commandName: 'create_character'
        });
    } catch (error) {
        console.error('Error creating character:', error);
        throw error;
    }
}

/**
 * Create a character with avatar
 * @param {Object} data - Character and avatar data
 * @returns {Promise<Object>} - Created character
 */
export async function createCharacterWithAvatar(data) {
    try {
        return await charactersClient.call('create-with-avatar', { dto: data }, {
            path: 'create-with-avatar',
            commandName: 'create_character_with_avatar'
        });
    } catch (error) {
        console.error('Error creating character with avatar:', error);
        throw error;
    }
}

/**
 * Update a character
 * @param {string} name - Character name
 * @param {Object} characterData - Character data to update
 * @returns {Promise<Object>} - Updated character
 */
export async function updateCharacter(name, characterData) {
    try {
        return await charactersClient.call('update', { name, dto: characterData }, {
            path: 'edit',
            commandName: 'update_character'
        });
    } catch (error) {
        console.error(`Error updating character ${name}:`, error);
        throw error;
    }
}

/**
 * Delete a character
 * @param {Object} dto - Delete character DTO
 * @returns {Promise<void>}
 */
export async function deleteCharacter(dto) {
    try {
        return await charactersClient.call('delete', { dto }, {
            path: 'delete',
            commandName: 'delete_character'
        });
    } catch (error) {
        console.error(`Error deleting character ${dto.name}:`, error);
        throw error;
    }
}

/**
 * Rename a character
 * @param {Object} dto - Rename character DTO
 * @returns {Promise<Object>} - Updated character
 */
export async function renameCharacter(dto) {
    try {
        return await charactersClient.call('rename', { dto }, {
            path: 'rename',
            commandName: 'rename_character'
        });
    } catch (error) {
        console.error(`Error renaming character ${dto.old_name} to ${dto.new_name}:`, error);
        throw error;
    }
}

/**
 * Import a character
 * @param {Object} dto - Import character DTO
 * @returns {Promise<Object>} - Imported character
 */
export async function importCharacter(dto) {
    try {
        return await charactersClient.call('import', { dto }, {
            path: 'import',
            commandName: 'import_character'
        });
    } catch (error) {
        console.error('Error importing character:', error);
        throw error;
    }
}

/**
 * Export a character
 * @param {Object} dto - Export character DTO
 * @returns {Promise<void>}
 */
export async function exportCharacter(dto) {
    try {
        return await charactersClient.call('export', { dto }, {
            path: 'export',
            commandName: 'export_character'
        });
    } catch (error) {
        console.error(`Error exporting character ${dto.name}:`, error);
        throw error;
    }
}

/**
 * Update a character's avatar
 * @param {Object} dto - Update avatar DTO
 * @returns {Promise<void>}
 */
export async function updateAvatar(dto) {
    try {
        return await charactersClient.call('update-avatar', { dto }, {
            path: 'update-avatar',
            commandName: 'update_avatar'
        });
    } catch (error) {
        console.error(`Error updating avatar for character ${dto.name}:`, error);
        throw error;
    }
}

/**
 * Get character chats
 * @param {Object} dto - Get character chats DTO
 * @returns {Promise<Array>} - List of character chats
 */
export async function getCharacterChats(dto) {
    try {
        return await charactersClient.call('get-chats', { dto }, {
            path: 'chats',
            commandName: 'get_character_chats_by_id'
        });
    } catch (error) {
        console.error(`Error getting chats for character ${dto.name}:`, error);
        throw error;
    }
}

/**
 * Clear character cache
 * @returns {Promise<void>}
 */
export async function clearCharacterCache() {
    try {
        return await charactersClient.call('clear-cache', {}, {
            path: 'clear-cache',
            commandName: 'clear_character_cache'
        });
    } catch (error) {
        console.error('Error clearing character cache:', error);
        throw error;
    }
}
