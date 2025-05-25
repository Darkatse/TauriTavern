// Tauri Themes Adapter for SillyTavern
// This module adapts the SillyTavern themes API to use Tauri backend

import * as ThemesAPI from '../scripts/tauri/themes-api.js';
import { isTauri } from '../tauri-bridge.js';

// Store original fetch function
let originalFetch = null;

/**
 * Initialize the themes adapter
 */
export function initializeThemesAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping themes adapter initialization');
        return;
    }

    console.log('Initializing Tauri themes adapter');

    try {
        // Store original fetch function if not already stored
        if (!originalFetch) {
            originalFetch = window.fetch;
        }

        // Override fetch for theme-related API calls
        window.fetch = async function(url, options = {}) {
            // Only intercept theme-related API calls
            if (typeof url === 'string') {
                // Handle theme API calls
                if (url === '/api/themes/all' && options.method === 'POST') {
                    console.debug('Intercepting themes/all API call');
                    try {
                        const themes = await ThemesAPI.getAllThemes();
                        return {
                            ok: true,
                            json: async () => themes,
                        };
                    } catch (error) {
                        console.error('Error in Tauri getAllThemes, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/themes/save' && options.method === 'POST') {
                    console.debug('Intercepting themes/save API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        const name = body.name;
                        
                        // Remove the name from the data since it's passed separately
                        const { name: _, ...data } = body;
                        
                        await ThemesAPI.saveTheme(name, data);
                        return {
                            ok: true,
                            json: async () => ({ success: true }),
                        };
                    } catch (error) {
                        console.error('Error in Tauri saveTheme, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }

                if (url === '/api/themes/delete' && options.method === 'POST') {
                    console.debug('Intercepting themes/delete API call');
                    try {
                        const bodyText = await new Response(options.body).text();
                        const body = JSON.parse(bodyText);
                        await ThemesAPI.deleteTheme(body.name);
                        return {
                            ok: true,
                            json: async () => ({ success: true }),
                        };
                    } catch (error) {
                        console.error('Error in Tauri deleteTheme, falling back to original:', error);
                        return originalFetch(url, options);
                    }
                }
            }

            // Call original fetch for non-theme API calls
            return originalFetch(url, options);
        };

        console.log('Tauri themes adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri themes adapter:', error);
    }
}
