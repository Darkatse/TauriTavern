// Tauri Chats Adapter for SillyTavern
// This module adapts the SillyTavern chats API to use Tauri backend

import * as ChatsAPI from '../scripts/tauri/chats-api.js';
import { isTauri } from '../tauri-bridge.js';

// Original functions to patch
let originalGetAllChats = null;
let originalGetChat = null;
let originalGetCharacterChats = null;
let originalCreateChat = null;
let originalAddMessage = null;
let originalRenameChat = null;
let originalDeleteChat = null;
let originalSearchChats = null;
let originalImportChat = null;
let originalExportChat = null;
let originalBackupChat = null;

/**
 * Initialize the chats adapter
 */
export function initializeChatsAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping chats adapter initialization');
        return;
    }

    console.log('Initializing Tauri chats adapter');

    try {
        // Get the chats module
        const chatsModule = window.chat;
        if (!chatsModule) {
            console.error('Chats module not found');
            return;
        }

        // Save original functions
        originalGetAllChats = chatsModule.getAllChats;
        originalGetChat = chatsModule.getChat;
        originalGetCharacterChats = chatsModule.getCharacterChats;
        originalCreateChat = chatsModule.createChat;
        originalAddMessage = chatsModule.addMessage;
        originalRenameChat = chatsModule.renameChat;
        originalDeleteChat = chatsModule.deleteChat;
        originalSearchChats = chatsModule.searchChats;
        originalImportChat = chatsModule.importChat;
        originalExportChat = chatsModule.exportChat;
        originalBackupChat = chatsModule.backupChat;

        // Patch the functions
        chatsModule.getAllChats = async function() {
            try {
                console.log('Using Tauri backend for getAllChats');
                return await ChatsAPI.getAllChats();
            } catch (error) {
                console.error('Error in Tauri getAllChats, falling back to original:', error);
                return originalGetAllChats.apply(this, arguments);
            }
        };

        chatsModule.getChat = async function(characterName, fileName) {
            try {
                console.log(`Using Tauri backend for getChat: ${characterName}/${fileName}`);
                return await ChatsAPI.getChat(characterName, fileName);
            } catch (error) {
                console.error('Error in Tauri getChat, falling back to original:', error);
                return originalGetChat.apply(this, arguments);
            }
        };

        chatsModule.getCharacterChats = async function(characterName) {
            try {
                console.log(`Using Tauri backend for getCharacterChats: ${characterName}`);
                return await ChatsAPI.getCharacterChats(characterName);
            } catch (error) {
                console.error('Error in Tauri getCharacterChats, falling back to original:', error);
                return originalGetCharacterChats.apply(this, arguments);
            }
        };

        chatsModule.createChat = async function(chatData) {
            try {
                console.log('Using Tauri backend for createChat');
                return await ChatsAPI.createChat(chatData);
            } catch (error) {
                console.error('Error in Tauri createChat, falling back to original:', error);
                return originalCreateChat.apply(this, arguments);
            }
        };

        chatsModule.addMessage = async function(messageData) {
            try {
                console.log('Using Tauri backend for addMessage');
                return await ChatsAPI.addMessage(messageData);
            } catch (error) {
                console.error('Error in Tauri addMessage, falling back to original:', error);
                return originalAddMessage.apply(this, arguments);
            }
        };

        chatsModule.renameChat = async function(dto) {
            try {
                console.log(`Using Tauri backend for renameChat: ${dto.character_name}/${dto.old_file_name} -> ${dto.new_file_name}`);
                return await ChatsAPI.renameChat(dto);
            } catch (error) {
                console.error('Error in Tauri renameChat, falling back to original:', error);
                return originalRenameChat.apply(this, arguments);
            }
        };

        chatsModule.deleteChat = async function(characterName, fileName) {
            try {
                console.log(`Using Tauri backend for deleteChat: ${characterName}/${fileName}`);
                return await ChatsAPI.deleteChat(characterName, fileName);
            } catch (error) {
                console.error('Error in Tauri deleteChat, falling back to original:', error);
                return originalDeleteChat.apply(this, arguments);
            }
        };

        chatsModule.searchChats = async function(query, characterFilter) {
            try {
                console.log(`Using Tauri backend for searchChats: ${query}`);
                return await ChatsAPI.searchChats(query, characterFilter);
            } catch (error) {
                console.error('Error in Tauri searchChats, falling back to original:', error);
                return originalSearchChats.apply(this, arguments);
            }
        };

        chatsModule.importChat = async function(dto) {
            try {
                console.log('Using Tauri backend for importChat');
                return await ChatsAPI.importChat(dto);
            } catch (error) {
                console.error('Error in Tauri importChat, falling back to original:', error);
                return originalImportChat.apply(this, arguments);
            }
        };

        chatsModule.exportChat = async function(dto) {
            try {
                console.log(`Using Tauri backend for exportChat: ${dto.character_name}/${dto.file_name}`);
                return await ChatsAPI.exportChat(dto);
            } catch (error) {
                console.error('Error in Tauri exportChat, falling back to original:', error);
                return originalExportChat.apply(this, arguments);
            }
        };

        chatsModule.backupChat = async function(characterName, fileName) {
            try {
                console.log(`Using Tauri backend for backupChat: ${characterName}/${fileName}`);
                return await ChatsAPI.backupChat(characterName, fileName);
            } catch (error) {
                console.error('Error in Tauri backupChat, falling back to original:', error);
                return originalBackupChat.apply(this, arguments);
            }
        };

        console.log('Tauri chats adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri chats adapter:', error);
    }
}
