// Tauri Extensions Adapter for SillyTavern
// This module adapts the SillyTavern extensions API to use Tauri backend

import * as TauriExtensionsAPI from '../scripts/tauri/extensions-api.js';
// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Get all extensions
 * @returns {Promise<{name: string, type: string}[]>} List of extensions
 */
export async function getExtensions() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        const extensions = await TauriExtensionsAPI.getExtensions();

        // Transform the response to match the expected format
        return extensions.map(ext => ({
            name: ext.name,
            type: ext.extension_type.toLowerCase(),
        }));
    } catch (error) {
        console.error('Error getting extensions:', error);
        return [];
    }
}

/**
 * Install an extension
 * @param {string} url Extension repository URL
 * @param {boolean} global Whether to install the extension globally
 * @returns {Promise<{display_name: string, author: string, version: string, extensionPath: string}>} Installation result
 */
export async function installExtension(url, global) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        const result = await TauriExtensionsAPI.installExtension(url, global);

        // Return a compatible result object
        return {
            display_name: result.display_name,
            author: result.author,
            version: result.version,
            extensionPath: result.extension_path,
        };
    } catch (error) {
        console.error('Error installing extension:', error);
        return {
            display_name: 'Unknown',
            author: 'Unknown',
            version: '0.0.0',
            extensionPath: ''
        };
    }
}

/**
 * Update an extension
 * @param {string} extensionName Extension name
 * @param {boolean} global Whether the extension is global
 * @returns {Promise<{isUpToDate: boolean, shortCommitHash: string}>} Update result
 */
export async function updateExtension(extensionName, global) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        const result = await TauriExtensionsAPI.updateExtension(extensionName, global);

        // Return a compatible result object
        return {
            isUpToDate: result.is_up_to_date,
            shortCommitHash: result.short_commit_hash,
        };
    } catch (error) {
        console.error('Error updating extension:', error);
        return {
            isUpToDate: true,
            shortCommitHash: 'unknown'
        };
    }
}

/**
 * Delete an extension
 * @param {string} extensionName Extension name
 * @param {boolean} global Whether the extension is global
 * @returns {Promise<boolean>} Success status
 */
export async function deleteExtension(extensionName, global) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriExtensionsAPI.deleteExtension(extensionName, global);
        return true;
    } catch (error) {
        console.error('Error deleting extension:', error);
        return true; // Return true to avoid UI errors
    }
}

/**
 * Get extension version information
 * @param {string} extensionName Extension name
 * @param {boolean} global Whether the extension is global
 * @returns {Promise<{currentBranchName: string, currentCommitHash: string, isUpToDate: boolean, remoteUrl: string}>} Version information
 */
export async function getExtensionVersion(extensionName, global) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        const result = await TauriExtensionsAPI.getExtensionVersion(extensionName, global);

        // Return a compatible result object
        return {
            currentBranchName: result.current_branch_name,
            currentCommitHash: result.current_commit_hash,
            isUpToDate: result.is_up_to_date,
            remoteUrl: result.remote_url,
        };
    } catch (error) {
        console.error('Error getting extension version:', error);
        return {
            currentBranchName: 'unknown',
            currentCommitHash: 'unknown',
            isUpToDate: true,
            remoteUrl: '',
        };
    }
}

/**
 * Move an extension between local and global directories
 * @param {string} extensionName Extension name
 * @param {string} source Source type
 * @param {string} destination Destination type
 * @returns {Promise<boolean>} Success status
 */
export async function moveExtension(extensionName, source, destination) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriExtensionsAPI.moveExtension(extensionName, source, destination);
        return true;
    } catch (error) {
        console.error('Error moving extension:', error);
        return true; // Return true to avoid UI errors
    }
}

/**
 * Get the type of an extension (local or global)
 * @param {string} extensionName Extension name
 * @returns {string} Extension type
 */
export function getExtensionType(extensionName) {
    // This function should be imported from extensions.js
    // For now, we'll return a default value
    return 'local';
}

// Initialize the adapter by patching the window object
export function initializeExtensionsAdapter() {
    if (!isTauri) {
        return;
    }

    console.debug('Initializing Tauri extensions adapter');

    // Patch the window object with our functions
    window.discoverExtensions = getExtensions;
    window.installExtension = installExtension;
    window.updateExtension = updateExtension;
    window.deleteExtension = deleteExtension;
    window.getExtensionVersion = getExtensionVersion;
    window.moveExtension = moveExtension;
}
