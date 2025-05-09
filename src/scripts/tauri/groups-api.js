// Tauri Groups API for SillyTavern
// This module provides API functions for managing groups in the Tauri backend

import { createApiClient } from './api-client.js';

// Create a groups API client
const groupsClient = createApiClient('groups');

/**
 * Get all groups
 * @returns {Promise<Array>} - List of groups
 */
export async function getAllGroups() {
    try {
        return await groupsClient.call('get-all', {}, {
            path: 'all',
            commandName: 'get_all_groups'
        });
    } catch (error) {
        console.error('Error getting all groups:', error);
        throw error;
    }
}

/**
 * Get a group by ID
 * @param {string} id - Group ID
 * @returns {Promise<Object>} - Group data
 */
export async function getGroup(id) {
    try {
        return await groupsClient.call('get', { id }, {
            path: 'get',
            commandName: 'get_group'
        });
    } catch (error) {
        console.error(`Error getting group ${id}:`, error);
        throw error;
    }
}

/**
 * Create a new group
 * @param {Object} groupData - Group data
 * @returns {Promise<Object>} - Created group
 */
export async function createGroup(groupData) {
    try {
        return await groupsClient.call('create', { dto: groupData }, {
            path: 'create',
            commandName: 'create_group'
        });
    } catch (error) {
        console.error('Error creating group:', error);
        throw error;
    }
}

/**
 * Update a group
 * @param {Object} groupData - Group data to update
 * @returns {Promise<Object>} - Updated group
 */
export async function updateGroup(groupData) {
    try {
        return await groupsClient.call('update', { dto: groupData }, {
            path: 'edit',
            commandName: 'update_group'
        });
    } catch (error) {
        console.error(`Error updating group ${groupData.id}:`, error);
        throw error;
    }
}

/**
 * Delete a group
 * @param {string} id - Group ID to delete
 * @returns {Promise<void>}
 */
export async function deleteGroup(id) {
    try {
        return await groupsClient.call('delete', { dto: { id } }, {
            path: 'delete',
            commandName: 'delete_group'
        });
    } catch (error) {
        console.error(`Error deleting group ${id}:`, error);
        throw error;
    }
}

/**
 * Get all group chat paths
 * @returns {Promise<Array<string>>} - List of group chat paths
 */
export async function getGroupChatPaths() {
    try {
        return await groupsClient.call('get-chat-paths', {}, {
            path: 'chat-paths',
            commandName: 'get_group_chat_paths'
        });
    } catch (error) {
        console.error('Error getting group chat paths:', error);
        throw error;
    }
}

/**
 * Clear the group cache
 * @returns {Promise<void>}
 */
export async function clearGroupCache() {
    try {
        return await groupsClient.call('clear-cache', {}, {
            path: 'clear-cache',
            commandName: 'clear_group_cache'
        });
    } catch (error) {
        console.error('Error clearing group cache:', error);
        throw error;
    }
}
