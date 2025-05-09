// Tauri Backgrounds API for SillyTavern
// This module provides API functions for managing backgrounds in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a backgrounds API client
const backgroundsClient = createApiClient('backgrounds');

/**
 * Get all backgrounds
 * @returns {Promise<Array<string>>} - List of background filenames
 */
export async function getAllBackgrounds() {
    try {
        // Backend now returns a simple array of filenames
        return await backgroundsClient.call('get-all', {}, {
            path: 'all',
            commandName: 'get_all_backgrounds'
        });
    } catch (error) {
        console.error('Error getting all backgrounds:', error);
        throw error;
    }
}

/**
 * Delete a background
 * @param {string} bg - Background filename to delete
 * @returns {Promise<void>}
 */
export async function deleteBackground(bg) {
    try {
        return await backgroundsClient.call('delete', { dto: { bg } }, {
            path: 'delete',
            commandName: 'delete_background'
        });
    } catch (error) {
        console.error(`Error deleting background ${bg}:`, error);
        throw error;
    }
}

/**
 * Rename a background
 * @param {string} oldBg - Current background filename
 * @param {string} newBg - New background filename
 * @returns {Promise<void>}
 */
export async function renameBackground(oldBg, newBg) {
    try {
        return await backgroundsClient.call('rename', { dto: { old_bg: oldBg, new_bg: newBg } }, {
            path: 'rename',
            commandName: 'rename_background'
        });
    } catch (error) {
        console.error(`Error renaming background from ${oldBg} to ${newBg}:`, error);
        throw error;
    }
}

/**
 * Upload a background
 * @param {string} filename - Background filename
 * @param {Uint8Array} data - Background image data
 * @returns {Promise<string>} - Uploaded background filename
 */
export async function uploadBackground(filename, data) {
    try {
        return await backgroundsClient.call('upload', { filename, data }, {
            path: 'upload',
            commandName: 'upload_background'
        });
    } catch (error) {
        console.error(`Error uploading background ${filename}:`, error);
        throw error;
    }
}
