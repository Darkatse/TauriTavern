// Tauri Groups Adapter for SillyTavern
// This module adapts the SillyTavern groups API to use Tauri backend

import * as GroupsAPI from '../scripts/tauri/groups-api.js';
import { isTauri } from '../tauri-bridge.js';

// Store original fetch function
let originalFetch = null;

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
        // We'll use a different approach that doesn't rely on window.groupChatsModule
        // Instead, we'll intercept API calls directly

        // Store original fetch function if not already stored
        if (!originalFetch) {
            originalFetch = window.fetch;
        }

        // Override fetch for group-related API calls
        window.fetch = async function(url, options = {}) {
            // Only intercept group-related API calls
            if (typeof url === 'string') {
                // Handle group API calls
                if (url === '/api/groups/all' && options.method === 'POST') {
                    console.debug('Intercepting groups/all API call');
                    try {
                        const groups = await GroupsAPI.getAllGroups();
                        return {
                            ok: true,
                            json: async () => groups,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getAllGroups, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/groups/create' && options.method === 'POST') {
                    console.debug('Intercepting groups/create API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const groupData = JSON.parse(bodyText);
                        const result = await GroupsAPI.createGroup(groupData);
                        return {
                            ok: true,
                            json: async () => result,
                        };
                    } catch (error) {
                        console.error('Error in Tauri createGroup, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/groups/edit' && options.method === 'POST') {
                    console.debug('Intercepting groups/edit API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const group = JSON.parse(bodyText);
                        await GroupsAPI.updateGroup(group);
                        return {
                            ok: true,
                            json: async () => ({ result: 'ok' }),
                        };
                    } catch (error) {
                        console.error('Error in Tauri updateGroup, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/groups/delete' && options.method === 'POST') {
                    console.debug('Intercepting groups/delete API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        await GroupsAPI.deleteGroup(body.id);
                        return {
                            ok: true,
                            json: async () => ({ result: 'ok' }),
                        };
                    } catch (error) {
                        console.error('Error in Tauri deleteGroup, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                // Add more group API endpoints as needed
            }

            // Call original fetch for non-group API calls
            return originalFetch(url, options);
        };

        console.log('Tauri groups adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri groups adapter:', error);
    }
}
