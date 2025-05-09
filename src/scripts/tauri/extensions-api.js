// Extensions API for Tauri
// This module provides functions for managing extensions in TauriTavern

import { createApiClient } from './api-client.js';

// Create API client for extensions
const extensionsApi = createApiClient('extensions');

/**
 * Get all extensions
 * @returns {Promise<Array>} - List of extensions
 */
export async function getExtensions() {
    return extensionsApi.call('get_extensions', {}, { commandName: 'get_extensions' });
}

/**
 * Install an extension from a URL
 * @param {string} url - URL of the extension repository
 * @param {boolean} global - Whether to install the extension globally
 * @returns {Promise<Object>} - Installation result
 */
export async function installExtension(url, global = false) {
    return extensionsApi.call('install_extension', { url, global }, { commandName: 'install_extension' });
}

/**
 * Update an extension
 * @param {string} extensionName - Name of the extension
 * @param {boolean} global - Whether the extension is global
 * @returns {Promise<Object>} - Update result
 */
export async function updateExtension(extensionName, global = false) {
    return extensionsApi.call('update_extension', { extension_name: extensionName, global }, { commandName: 'update_extension' });
}

/**
 * Delete an extension
 * @param {string} extensionName - Name of the extension
 * @param {boolean} global - Whether the extension is global
 * @returns {Promise<void>}
 */
export async function deleteExtension(extensionName, global = false) {
    return extensionsApi.call('delete_extension', { extension_name: extensionName, global }, { commandName: 'delete_extension' });
}

/**
 * Get extension version information
 * @param {string} extensionName - Name of the extension
 * @param {boolean} global - Whether the extension is global
 * @returns {Promise<Object>} - Version information
 */
export async function getExtensionVersion(extensionName, global = false) {
    return extensionsApi.call('get_extension_version', { extension_name: extensionName, global }, { commandName: 'get_extension_version' });
}

/**
 * Move an extension between local and global directories
 * @param {string} extensionName - Name of the extension
 * @param {string} source - Source location ('local' or 'global')
 * @param {string} destination - Destination location ('local' or 'global')
 * @returns {Promise<void>}
 */
export async function moveExtension(extensionName, source, destination) {
    return extensionsApi.call('move_extension', { extension_name: extensionName, source, destination }, { commandName: 'move_extension' });
}

// Export all functions as a module
export default {
    getExtensions,
    installExtension,
    updateExtension,
    deleteExtension,
    getExtensionVersion,
    moveExtension
};
