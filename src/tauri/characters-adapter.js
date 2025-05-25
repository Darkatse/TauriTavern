// Tauri Characters Adapter for SillyTavern
// This module adapts the SillyTavern characters API to use Tauri backend

import * as CharactersAPI from '../scripts/tauri/characters-api.js';
import { isTauri } from '../tauri-bridge.js';

// Store original fetch function
let originalFetch = null;

/**
 * Initialize the characters adapter
 */
export function initializeCharactersAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping characters adapter initialization');
        return;
    }

    console.log('Initializing Tauri characters adapter');

    try {
        // We'll use a different approach that doesn't rely on window.characters
        // Instead, we'll intercept API calls directly

        // Store original fetch function if not already stored
        if (!originalFetch) {
            originalFetch = window.fetch;
        }

        // Override fetch for character-related API calls
        window.fetch = async function(url, options = {}) {
            // Only intercept character-related API calls
            if (typeof url === 'string') {
                // Handle character API calls
                if (url === '/api/characters/all' && options.method === 'POST') {
                    console.debug('Intercepting characters/all API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const shallow = body.shallow !== false; // Default to true if not specified
                        const characters = await CharactersAPI.getAllCharacters(shallow);
                        return {
                            ok: true,
                            json: async () => characters,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getAllCharacters, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/get' && options.method === 'POST') {
                    console.debug('Intercepting characters/get API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const character = await CharactersAPI.getCharacter(body.name);
                        return {
                            ok: true,
                            json: async () => character,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/create' && options.method === 'POST') {
                    console.debug('Intercepting characters/create API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const characterData = JSON.parse(bodyText);
                        const result = await CharactersAPI.createCharacter(characterData);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri createCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/edit' && options.method === 'POST') {
                    console.debug('Intercepting characters/edit API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const result = await CharactersAPI.updateCharacter(body.name, body);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri updateCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/delete' && options.method === 'POST') {
                    console.debug('Intercepting characters/delete API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await CharactersAPI.deleteCharacter(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri deleteCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/rename' && options.method === 'POST') {
                    console.debug('Intercepting characters/rename API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await CharactersAPI.renameCharacter(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri renameCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/import' && options.method === 'POST') {
                    console.debug('Intercepting characters/import API call');
                    try {
                        // Handle FormData for character import
                        const formData = options.body;
                        if (!(formData instanceof FormData)) {
                            return originalFetch(url, options);
                        }

                        // Extract character data
                        const file = formData.get('avatar');
                        const name = formData.get('name');
                        const formDataObject = {
                            file: file instanceof File ? file : null,
                            name: name || '',
                        };

                        // Import character
                        const result = await CharactersAPI.importCharacter(formDataObject);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri importCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/export' && options.method === 'POST') {
                    console.debug('Intercepting characters/export API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await CharactersAPI.exportCharacter(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri exportCharacter, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/avatar' && options.method === 'POST') {
                    console.debug('Intercepting characters/avatar API call');
                    try {
                        // Handle FormData for avatar update
                        const formData = options.body;
                        if (!(formData instanceof FormData)) {
                            return originalFetch(url, options);
                        }

                        // Extract avatar data
                        const file = formData.get('avatar');
                        const name = formData.get('name');
                        const formDataObject = {
                            file: file instanceof File ? file : null,
                            name: name || '',
                        };

                        // Update avatar
                        const result = await CharactersAPI.updateAvatar(formDataObject);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri updateAvatar, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/characters/chats' && options.method === 'POST') {
                    console.debug('Intercepting characters/chats API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const dto = JSON.parse(bodyText);
                        const result = await CharactersAPI.getCharacterChats(dto);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getCharacterChats, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }
            }

            // Call original fetch for non-character API calls
            return originalFetch(url, options);
        };

        console.log('Tauri characters adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri characters adapter:', error);
    }
}
