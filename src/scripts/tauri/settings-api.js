// Tauri Settings API for SillyTavern
// This module provides API functions for managing settings in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a settings API client
const settingsClient = createApiClient('settings');

/**
 * Get application settings
 * @returns {Promise<Object>} - Application settings
 */
export async function getSettings() {
    try {
        return await settingsClient.call('get', {}, {
            path: 'get',
            commandName: 'get_settings'
        });
    } catch (error) {
        console.error('Error getting settings:', error);
        throw error;
    }
}

/**
 * Update application settings
 * @param {Object} settingsData - Settings data to update
 * @returns {Promise<Object>} - Updated settings
 */
export async function updateSettings(settingsData) {
    try {
        return await settingsClient.call('update', { dto: settingsData }, {
            path: 'update',
            commandName: 'update_settings'
        });
    } catch (error) {
        console.error('Error updating settings:', error);
        throw error;
    }
}

/**
 * Save user settings
 * @param {Object} settings - User settings to save
 * @returns {Promise<void>}
 */
export async function saveUserSettings(settings) {
    try {
        return await settingsClient.call('save', { settings }, {
            path: 'save',
            commandName: 'save_user_settings'
        });
    } catch (error) {
        console.error('Error saving user settings:', error);
        throw error;
    }
}

/**
 * Get SillyTavern settings
 * @returns {Promise<Object>} - SillyTavern settings
 */
export async function getSillyTavernSettings() {
    try {
        return await settingsClient.call('get', {}, {
            path: 'get',
            commandName: 'get_sillytavern_settings'
        });
    } catch (error) {
        console.error('Error getting SillyTavern settings:', error);
        throw error;
    }
}

/**
 * Create settings snapshot
 * @returns {Promise<void>}
 */
export async function createSettingsSnapshot() {
    try {
        return await settingsClient.call('create-snapshot', {}, {
            path: 'make-snapshot',
            commandName: 'create_settings_snapshot'
        });
    } catch (error) {
        console.error('Error creating settings snapshot:', error);
        throw error;
    }
}

/**
 * Get settings snapshots
 * @returns {Promise<Array>} - List of snapshots
 */
export async function getSettingsSnapshots() {
    try {
        return await settingsClient.call('get-snapshots', {}, {
            path: 'get-snapshots',
            commandName: 'get_settings_snapshots'
        });
    } catch (error) {
        console.error('Error getting settings snapshots:', error);
        throw error;
    }
}

/**
 * Load settings snapshot
 * @param {string} name - Snapshot name
 * @returns {Promise<Object>} - Snapshot settings
 */
export async function loadSettingsSnapshot(name) {
    try {
        return await settingsClient.call('load-snapshot', { name }, {
            path: 'load-snapshot',
            commandName: 'load_settings_snapshot'
        });
    } catch (error) {
        console.error('Error loading settings snapshot:', error);
        throw error;
    }
}

/**
 * Restore settings snapshot
 * @param {string} name - Snapshot name
 * @returns {Promise<void>}
 */
export async function restoreSettingsSnapshot(name) {
    try {
        return await settingsClient.call('restore-snapshot', { name }, {
            path: 'restore-snapshot',
            commandName: 'restore_settings_snapshot'
        });
    } catch (error) {
        console.error('Error restoring settings snapshot:', error);
        throw error;
    }
}
