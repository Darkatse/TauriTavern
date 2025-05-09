// Tauri Settings Adapter for SillyTavern
// This module adapts the SillyTavern settings API to use Tauri backend

import * as TauriSettingsAPI from '../scripts/tauri/settings-api.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Get SillyTavern settings
 * This function is called by the frontend to get all settings
 * @returns {Promise<Object>} - SillyTavern settings
 */
export async function getSettings() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        const settings = await TauriSettingsAPI.getSillyTavernSettings();
        return settings;
    } catch (error) {
        console.error('Error getting SillyTavern settings:', error);
        throw error;
    }
}

/**
 * Save SillyTavern settings
 * This function is called by the frontend to save all settings
 * @param {Object} settings - SillyTavern settings
 * @returns {Promise<Object>} - Result
 */
export async function saveSettings(settings) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriSettingsAPI.saveUserSettings(settings);
        return { result: 'ok' };
    } catch (error) {
        console.error('Error saving SillyTavern settings:', error);
        throw error;
    }
}

/**
 * Create settings snapshot
 * @returns {Promise<void>}
 */
export async function createSnapshot() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriSettingsAPI.createSettingsSnapshot();
        return { result: 'ok' };
    } catch (error) {
        console.error('Error creating settings snapshot:', error);
        throw error;
    }
}

/**
 * Get settings snapshots
 * @returns {Promise<Array>} - List of snapshots
 */
export async function getSnapshots() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        return await TauriSettingsAPI.getSettingsSnapshots();
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
export async function loadSnapshot(name) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        return await TauriSettingsAPI.loadSettingsSnapshot(name);
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
export async function restoreSnapshot(name) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }

    try {
        await TauriSettingsAPI.restoreSettingsSnapshot(name);
        return { result: 'ok' };
    } catch (error) {
        console.error('Error restoring settings snapshot:', error);
        throw error;
    }
}
