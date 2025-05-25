// Tauri Chats Adapter for SillyTavern
// This module adapts the SillyTavern chats API to use Tauri backend

import * as ChatsAPI from '../scripts/tauri/chats-api.js';
import { isTauri } from '../tauri-bridge.js';

// Store original fetch function
let originalFetch = null;

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
        // We'll use a different approach that doesn't rely on window.chat
        // Instead, we'll intercept API calls directly

        // Store original fetch function if not already stored
        if (!originalFetch) {
            originalFetch = window.fetch;
        }

        // Override fetch for chat-related API calls
        window.fetch = async function(url, options = {}) {
            // Only intercept chat-related API calls
            if (typeof url === 'string') {
                // Handle chat API calls
                if (url === '/api/chats/all' && options.method === 'POST') {
                    console.debug('Intercepting chats/all API call');
                    try {
                        const chats = await ChatsAPI.getAllChats();
                        return {
                            ok: true,
                            json: async () => chats,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getAllChats, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/get' && options.method === 'POST') {
                    console.debug('Intercepting chats/get API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const chat = await ChatsAPI.getChat(body.character_name, body.file_name);
                        return {
                            ok: true,
                            json: async () => chat,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/character' && options.method === 'POST') {
                    console.debug('Intercepting chats/character API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const chats = await ChatsAPI.getCharacterChats(body.character_name);
                        return {
                            ok: true,
                            json: async () => chats,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getCharacterChats, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/create' && options.method === 'POST') {
                    console.debug('Intercepting chats/create API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const chatData = JSON.parse(bodyText);
                        const result = await ChatsAPI.createChat(chatData);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri createChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/message' && options.method === 'POST') {
                    console.debug('Intercepting chats/message API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const messageData = JSON.parse(bodyText);
                        const result = await ChatsAPI.addMessage(messageData);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri addMessage, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/rename' && options.method === 'POST') {
                    console.debug('Intercepting chats/rename API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await ChatsAPI.renameChat(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri renameChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/delete' && options.method === 'POST') {
                    console.debug('Intercepting chats/delete API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const result = await ChatsAPI.deleteChat(body.character_name, body.file_name);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri deleteChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/search' && options.method === 'POST') {
                    console.debug('Intercepting chats/search API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const results = await ChatsAPI.searchChats(body.query, body.character_filter);
                        return {
                            ok: true,
                            json: async () => results,
                        };
                    } catch (error) {
                        console.error('Error in Tauri searchChats, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/import' && options.method === 'POST') {
                    console.debug('Intercepting chats/import API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await ChatsAPI.importChat(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri importChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/export' && options.method === 'POST') {
                    console.debug('Intercepting chats/export API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await ChatsAPI.exportChat(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri exportChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/chats/backup' && options.method === 'POST') {
                    console.debug('Intercepting chats/backup API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const result = await ChatsAPI.backupChat(body.character_name, body.file_name);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri backupChat, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }
            }

            // Call original fetch for non-chat API calls
            return originalFetch(url, options);
        };

        console.log('Tauri chats adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri chats adapter:', error);
    }
}
