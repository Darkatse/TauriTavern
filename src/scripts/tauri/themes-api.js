// Tauri Themes API for SillyTavern
// This module provides API functions for managing themes in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a themes API client
const themesClient = createApiClient('themes');

/**
 * Get all themes
 * @returns {Promise<Array>} - List of themes
 */
export async function getAllThemes() {
    try {
        return await themesClient.call('get-all', {}, {
            path: 'all',
            commandName: 'get_all_themes'
        });
    } catch (error) {
        console.error('Error getting all themes:', error);
        throw error;
    }
}

/**
 * Save a theme
 * @param {string} name - Theme name
 * @param {Object} data - Theme data
 * @returns {Promise<void>}
 */
export async function saveTheme(name, data) {
    try {
        // Create a DTO with name and data
        const dto = {
            name: name,
            ...data
        };
        
        return await themesClient.call('save', { dto }, {
            path: 'save',
            commandName: 'save_theme'
        });
    } catch (error) {
        console.error(`Error saving theme ${name}:`, error);
        throw error;
    }
}

/**
 * Delete a theme
 * @param {string} name - Theme name to delete
 * @returns {Promise<void>}
 */
export async function deleteTheme(name) {
    try {
        return await themesClient.call('delete', { dto: { name } }, {
            path: 'delete',
            commandName: 'delete_theme'
        });
    } catch (error) {
        console.error(`Error deleting theme ${name}:`, error);
        throw error;
    }
}
