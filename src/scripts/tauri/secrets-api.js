// Tauri Secrets API for SillyTavern
// This module provides API functions for managing secrets in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a secrets API client
const secretsClient = createApiClient('secrets');

/**
 * Write a secret to the backend
 * @param {string} key - The key of the secret
 * @param {string} value - The value of the secret
 * @returns {Promise<string>} - 'ok' if successful
 */
export async function writeSecret(key, value) {
    try {
        const result = await secretsClient.call('write', { dto: { key, value } }, {
            path: 'write',
            commandName: 'write_secret'
        });
        return result;
    } catch (error) {
        console.error('Error writing secret:', error);
        throw error;
    }
}

/**
 * Read the state of all secrets
 * @returns {Promise<Object>} - Object with secret keys and their state (boolean)
 */
export async function readSecretState() {
    try {
        const result = await secretsClient.call('read', {}, {
            path: 'read',
            commandName: 'read_secret_state'
        });
        return result.states;
    } catch (error) {
        console.error('Error reading secret state:', error);
        throw error;
    }
}

/**
 * View all secrets (requires allowKeysExposure to be true)
 * @returns {Promise<Object>} - Object with all secrets
 */
export async function viewSecrets() {
    try {
        const result = await secretsClient.call('view', {}, {
            path: 'view',
            commandName: 'view_secrets'
        });
        return result.secrets;
    } catch (error) {
        console.error('Error viewing secrets:', error);
        throw error;
    }
}

/**
 * Find a specific secret by key
 * @param {string} key - The key to find
 * @returns {Promise<string>} - The secret value
 */
export async function findSecret(key) {
    try {
        const result = await secretsClient.call('find', { dto: { key } }, {
            path: 'find',
            commandName: 'find_secret'
        });
        return result.value;
    } catch (error) {
        console.error('Error finding secret:', error);
        throw error;
    }
}
