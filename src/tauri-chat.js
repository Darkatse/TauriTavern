// Tauri Chat Module for SillyTavern
// This module provides chat-related functions for communicating with the Tauri backend

import * as TauriBridge from './tauri-bridge.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Gets a chat by ID
 * @param {string} chatId Chat ID
 * @returns {Promise<object|null>} Chat object or null if not found
 */
export async function getChat(chatId) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chat = await TauriBridge.getChat(chatId);
        return chat;
    } catch (error) {
        console.error(`Error getting chat ${chatId} from Tauri backend:`, error);
        return null;
    }
}

/**
 * Gets all chats for a character
 * @param {string} characterId Character ID
 * @returns {Promise<Array|null>} Array of chat objects or null if not found
 */
export async function getChatsByCharacter(characterId) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chats = await TauriBridge.getChatsByCharacter(characterId);
        return chats;
    } catch (error) {
        console.error(`Error getting chats for character ${characterId} from Tauri backend:`, error);
        return null;
    }
}

/**
 * Creates a new chat
 * @param {object} chatData Chat data
 * @returns {Promise<object|null>} Created chat object or null if creation failed
 */
export async function createChat(chatData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chat = await TauriBridge.createChat(chatData);
        return chat;
    } catch (error) {
        console.error('Error creating chat in Tauri backend:', error);
        return null;
    }
}

/**
 * Adds a message to a chat
 * @param {object} messageData Message data
 * @returns {Promise<object|null>} Updated chat object or null if addition failed
 */
export async function addMessage(messageData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chat = await TauriBridge.addMessage(messageData);
        return chat;
    } catch (error) {
        console.error('Error adding message in Tauri backend:', error);
        return null;
    }
}

/**
 * Updates a chat
 * @param {object} chatData Chat data
 * @returns {Promise<object|null>} Updated chat object or null if update failed
 */
export async function updateChat(chatData) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chat = await TauriBridge.updateChat(chatData);
        return chat;
    } catch (error) {
        console.error('Error updating chat in Tauri backend:', error);
        return null;
    }
}

/**
 * Deletes a chat
 * @param {string} chatId Chat ID
 * @returns {Promise<boolean>} True if deletion was successful, false otherwise
 */
export async function deleteChat(chatId) {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        await TauriBridge.deleteChat(chatId);
        return true;
    } catch (error) {
        console.error(`Error deleting chat ${chatId} from Tauri backend:`, error);
        return false;
    }
}

/**
 * Gets all chats
 * @returns {Promise<Array|null>} Array of chat objects or null if not found
 */
export async function getAllChats() {
    if (!isTauri) {
        return null; // Let SillyTavern handle it
    }
    
    try {
        const chats = await TauriBridge.getAllChats();
        return chats;
    } catch (error) {
        console.error('Error getting all chats from Tauri backend:', error);
        return null;
    }
}

/**
 * Checks if we're running in a Tauri environment
 * @returns {boolean} True if running in Tauri, false otherwise
 */
export function isTauriEnvironment() {
    return isTauri;
}
