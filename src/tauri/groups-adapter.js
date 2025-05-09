// Tauri Groups Adapter for SillyTavern
// This module adapts the SillyTavern groups API to use Tauri backend

import * as GroupsAPI from '../scripts/tauri/groups-api.js';
import { isTauri } from '../tauri-bridge.js';

// Original functions to patch
let originalGetGroups = null;
let originalCreateGroup = null;
let originalEditGroup = null;
let originalDeleteGroup = null;

/**
 * Initialize the groups adapter
 */
export function initializeGroupsAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping groups adapter initialization');
        return;
    }

    console.log('Initializing Tauri groups adapter');

    try {
        // Get the group-chats module
        const groupChatsModule = window.groupChatsModule;
        if (!groupChatsModule) {
            console.error('Group chats module not found');
            return;
        }

        // Save original functions
        originalGetGroups = groupChatsModule.getGroups;
        originalCreateGroup = groupChatsModule.createGroup;
        originalEditGroup = groupChatsModule._save; // This is the internal function used for editing groups
        originalDeleteGroup = groupChatsModule.deleteGroup;

        // Patch the functions
        groupChatsModule.getGroups = async function() {
            try {
                console.log('Using Tauri backend for getGroups');
                return await GroupsAPI.getAllGroups();
            } catch (error) {
                console.error('Error in Tauri getGroups, falling back to original:', error);
                return originalGetGroups.apply(this, arguments);
            }
        };

        groupChatsModule.createGroup = async function(groupData) {
            try {
                console.log('Using Tauri backend for createGroup');
                return await GroupsAPI.createGroup(groupData);
            } catch (error) {
                console.error('Error in Tauri createGroup, falling back to original:', error);
                return originalCreateGroup.apply(this, arguments);
            }
        };

        groupChatsModule._save = async function(group, reload = true) {
            try {
                console.log(`Using Tauri backend for editGroup: ${group.id}`);
                await GroupsAPI.updateGroup(group);
                if (reload) {
                    await getCharacters();
                }
            } catch (error) {
                console.error('Error in Tauri editGroup, falling back to original:', error);
                return originalEditGroup.apply(this, arguments);
            }
        };

        groupChatsModule.deleteGroup = async function(id) {
            try {
                console.log(`Using Tauri backend for deleteGroup: ${id}`);
                await GroupsAPI.deleteGroup(id);
                return true;
            } catch (error) {
                console.error('Error in Tauri deleteGroup, falling back to original:', error);
                return originalDeleteGroup.apply(this, arguments);
            }
        };

        console.log('Tauri groups adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri groups adapter:', error);
    }
}
