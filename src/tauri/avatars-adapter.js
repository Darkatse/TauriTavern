// Tauri Avatars Adapter for SillyTavern
// This module adapts the SillyTavern avatars API to use Tauri backend

import * as TauriAvatarsAPI from '../scripts/tauri/avatars-api.js';
// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Get all avatars
 * @returns {Promise<string[]>} List of avatar filenames
 */
export async function getAvatars() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        return await TauriAvatarsAPI.getAvatars();
    } catch (error) {
        console.error('Error getting avatars:', error);
        throw error;
    }
}

/**
 * Delete an avatar
 * @param {string} avatar Avatar filename to delete
 * @returns {Promise<{result: string}>} Result of the operation
 */
export async function deleteAvatar(avatar) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriAvatarsAPI.deleteAvatar(avatar);
        return { result: 'ok' };
    } catch (error) {
        console.error('Error deleting avatar:', error);
        throw error;
    }
}

/**
 * Upload an avatar
 * @param {File} file Avatar file to upload
 * @param {Object} options Upload options
 * @param {string} [options.overwriteName] Name to overwrite the avatar with
 * @param {Object} [options.crop] Crop information
 * @returns {Promise<{path: string}>} Path to the uploaded avatar
 */
export async function uploadAvatar(file, options = {}) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        // Create a temporary file path
        const tempFilePath = await window.__TAURI__.path.tempDir();
        const fileName = `avatar_upload_${Date.now()}.png`;
        const filePath = await window.__TAURI__.path.join(tempFilePath, fileName);

        // Write the file to disk
        const arrayBuffer = await file.arrayBuffer();
        const uint8Array = new Uint8Array(arrayBuffer);
        await window.__TAURI__.fs.writeBinaryFile(filePath, uint8Array);

        // Upload the avatar
        const result = await TauriAvatarsAPI.uploadAvatar(filePath, {
            overwriteName: options.overwriteName,
            crop: options.crop,
        });

        // Clean up the temporary file
        try {
            await window.__TAURI__.fs.removeFile(filePath);
        } catch (e) {
            console.warn('Failed to remove temporary file:', e);
        }

        return { path: result.path };
    } catch (error) {
        console.error('Error uploading avatar:', error);
        throw error;
    }
}

// Initialize the adapter by patching the fetch API
export function initializeAvatarsAdapter() {
    if (!isTauri) {
        return;
    }

    console.debug('Initializing Tauri avatars adapter');

    // Store original fetch function
    const originalFetch = window.fetch;

    // Override fetch for avatar-related API calls
    // @ts-ignore - TypeScript doesn't like us replacing fetch
    window.fetch = async function(url, options = {}) {
        // Only intercept avatar-related API calls
        if (typeof url === 'string') {
            // Handle avatar API calls
            if (url === '/api/avatars/get' && options.method === 'POST') {
                console.debug('Intercepting avatars/get API call');
                const avatars = await getAvatars();
                return {
                    ok: true,
                    json: async () => avatars,
                };
            }

            if (url === '/api/avatars/delete' && options.method === 'POST') {
                console.debug('Intercepting avatars/delete API call');
                const bodyText = await new Response(options.body).text();
                const body = JSON.parse(bodyText);
                const result = await deleteAvatar(body.avatar);
                return {
                    ok: true,
                    json: async () => result,
                };
            }

            if (url === '/api/avatars/upload' && options.method === 'POST') {
                console.debug('Intercepting avatars/upload API call');

                // Extract form data
                const formData = options.body;
                if (!(formData instanceof FormData)) {
                    return originalFetch(url, options);
                }

                // Get the avatar file
                const avatarFile = formData.get('avatar');
                if (!avatarFile || !(avatarFile instanceof File)) {
                    return originalFetch(url, options);
                }

                // Parse crop information if available
                let crop = null;
                const cropParam = new URL(url, window.location.origin).searchParams.get('crop');
                if (cropParam) {
                    crop = JSON.parse(decodeURIComponent(cropParam));
                }

                // Get overwrite name if available
                const overwriteName = formData.get('overwrite_name')
                    ? String(formData.get('overwrite_name'))
                    : undefined;

                // Upload the avatar
                const result = await uploadAvatar(avatarFile, {
                    overwriteName,
                    crop,
                });

                return {
                    ok: true,
                    json: async () => result,
                };
            }
        }

        // Call original fetch for non-avatar API calls
        return originalFetch(url, options);
    };
}
