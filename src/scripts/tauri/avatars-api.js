// Avatars API for Tauri
// This module provides functions for managing user avatars in TauriTavern

import { createApiClient } from './api-client.js';

// Create API client for avatars
const avatarsApi = createApiClient('avatars');

/**
 * Get all avatars
 * @returns {Promise<Array<string>>} - List of avatar filenames
 */
export async function getAvatars() {
    return avatarsApi.call('get_avatars', {}, { commandName: 'get_avatars' });
}

/**
 * Delete an avatar
 * @param {string} avatar - Avatar filename to delete
 * @returns {Promise<void>}
 */
export async function deleteAvatar(avatar) {
    return avatarsApi.call('delete_avatar', { avatar }, { commandName: 'delete_avatar' });
}

/**
 * Upload an avatar
 * @param {string} filePath - Path to the avatar file
 * @param {Object} [options] - Optional settings
 * @param {string} [options.overwriteName] - Name to overwrite the avatar with
 * @param {Object} [options.crop] - Crop information
 * @param {number} [options.crop.x] - X coordinate of the crop
 * @param {number} [options.crop.y] - Y coordinate of the crop
 * @param {number} [options.crop.width] - Width of the crop
 * @param {number} [options.crop.height] - Height of the crop
 * @returns {Promise<Object>} - Upload result
 */
export async function uploadAvatar(filePath, { overwriteName, crop } = {}) {
    const params = { file_path: filePath };

    if (overwriteName) {
        params.overwrite_name = overwriteName;
    }

    if (crop) {
        params.crop = JSON.stringify(crop);
    }

    return avatarsApi.call('upload_avatar', params, { commandName: 'upload_avatar' });
}

// Export all functions as a module
export default {
    getAvatars,
    deleteAvatar,
    uploadAvatar
};
