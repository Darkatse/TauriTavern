// Tauri Integration for SillyTavern
// This module provides integration between SillyTavern and the Tauri backend

import * as TauriBridge from './tauri-bridge.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

// Store the CSRF token
let token = null;

// Initialize the application
export async function initializeApp() {
    console.log('Initializing TauriTavern integration...');

    if (isTauri) {
        try {
            // Initialize the Tauri bridge
            const bridgeInitialized = await TauriBridge.initializeBridge();
            if (!bridgeInitialized) {
                console.error('Failed to initialize Tauri bridge');
                return false;
            }

            // Get CSRF token (dummy in Tauri, but kept for compatibility)
            token = await TauriBridge.getCsrfToken();

            // Initialize the application
            const appInitialized = await TauriBridge.initializeApp();
            if (!appInitialized) {
                console.error('Failed to initialize Tauri application');
                return false;
            }

            console.log('TauriTavern integration initialized successfully');
            return true;
        } catch (error) {
            console.error('Error initializing TauriTavern integration:', error);
            return false;
        }
    } else {
        console.log('Not running in Tauri environment, skipping Tauri integration');
        return false;
    }
}

// Get the CSRF token
export function getToken() {
    return token;
}

// Patch the fetch function to use Tauri API when appropriate
export function patchFetch() {
    if (!isTauri) {
        return;
    }

    console.log('Patching fetch function for Tauri integration');

    // Store the original fetch function
    const originalFetch = window.fetch;

    // Replace fetch with our custom implementation
    // @ts-ignore - TypeScript doesn't like us replacing fetch
    window.fetch = async function(url, options = {}) {
        // If the URL starts with http:// or https://, use the original fetch
        if (typeof url === 'string' && (url.startsWith('http://') || url.startsWith('https://'))) {
            return originalFetch(url, options);
        }

        // For API endpoints, use the Tauri bridge
        try {
            // Extract the endpoint path
            const path = typeof url === 'string' ? url : url.toString();

            // Handle specific API endpoints
            if (path === '/csrf-token') {
                return {
                    json: async () => ({ token: 'tauri-dummy-token' })
                };
            }

            if (path === '/api/characters/all' || path === '/getcharacters') {
                const characters = await TauriBridge.getAllCharacters();
                return {
                    json: async () => characters
                };
            }

            if (path.startsWith('/api/characters/get') || path.startsWith('/getonecharacter')) {
                const id = new URL(path, window.location.origin).searchParams.get('id');
                const character = await TauriBridge.getCharacter(id);
                return {
                    json: async () => character
                };
            }

            // For other endpoints, use the original fetch for now
            return originalFetch(url, options);
        } catch (error) {
            console.error('Error in patched fetch:', error);
            throw error;
        }
    };
}

// Patch XMLHttpRequest to use Tauri API when appropriate
export function patchXHR() {
    if (!isTauri) {
        return;
    }

    console.log('Patching XMLHttpRequest for Tauri integration');

    // Store the original XMLHttpRequest
    const OriginalXHR = window.XMLHttpRequest;

    // Replace XMLHttpRequest with our custom implementation
    // @ts-ignore - TypeScript doesn't like us replacing XMLHttpRequest
    window.XMLHttpRequest = function() {
        const xhr = new OriginalXHR();
        const originalOpen = xhr.open;
        const originalSend = xhr.send;

        // Override the open method
        // @ts-ignore - TypeScript doesn't like us adding properties to xhr
        xhr.open = function(method, url, async, user, password) {
            // Store the URL for later use
            // @ts-ignore - TypeScript doesn't like us adding properties to xhr
            xhr._url = url;
            // @ts-ignore - TypeScript doesn't like us adding properties to xhr
            xhr._method = method;

            // Call the original open method
            // @ts-ignore - TypeScript doesn't like us using apply with arguments
            return originalOpen.apply(xhr, arguments);
        };

        // Override the send method
        // @ts-ignore - TypeScript doesn't like us adding properties to xhr
        xhr.send = function(body) {
            // If the URL starts with http:// or https://, use the original send
            // @ts-ignore - TypeScript doesn't like us adding properties to xhr
            if (typeof xhr._url === 'string' && (xhr._url.startsWith('http://') || xhr._url.startsWith('https://'))) {
                // @ts-ignore - TypeScript doesn't like us using apply with arguments
                return originalSend.apply(xhr, arguments);
            }

            // For API endpoints, use the Tauri bridge
            try {
                // Extract the endpoint path
                // @ts-ignore - TypeScript doesn't like us adding properties to xhr
                const path = xhr._url;

                // Handle specific API endpoints
                if (path === '/csrf-token') {
                    // Simulate a successful response
                    setTimeout(() => {
                        // @ts-ignore - TypeScript doesn't like us setting readonly properties
                        xhr.status = 200;
                        // @ts-ignore - TypeScript doesn't like us setting readonly properties
                        xhr.responseText = JSON.stringify({ token: 'tauri-dummy-token' });
                        // @ts-ignore - TypeScript doesn't like us calling onload without arguments
                        xhr.onload && xhr.onload();
                    }, 0);
                    return;
                }

                // For other endpoints, use the original send for now
                // @ts-ignore - TypeScript doesn't like us using apply with arguments
                return originalSend.apply(xhr, arguments);
            } catch (error) {
                console.error('Error in patched XHR:', error);
                throw error;
            }
        };

        return xhr;
    };
}

// Initialize the integration
export async function initialize() {
    const appInitialized = await initializeApp();

    if (appInitialized) {
        // Patch fetch and XHR
        patchFetch();
        patchXHR();

        // Add event listeners for Tauri events
        TauriBridge.addEventListener('CharacterCreated', (data) => {
            console.log('Character created:', data);
            // Refresh character list if needed
            // We check if the function exists in the global scope
            // @ts-ignore - TypeScript doesn't know about global functions
            if (typeof window.getCharacters === 'function') {
                // @ts-ignore - TypeScript doesn't know about global functions
                window.getCharacters();
            }
        });

        TauriBridge.addEventListener('CharacterUpdated', (data) => {
            console.log('Character updated:', data);
            // Refresh character if it's the current one
            // We check if the function exists in the global scope
            // @ts-ignore - TypeScript doesn't know about global functions
            if (typeof window.getCurrentCharacter === 'function' &&
                // @ts-ignore - TypeScript doesn't know about global functions
                data.id === window.getCurrentCharacter()?.id) {
                // @ts-ignore - TypeScript doesn't know about global functions
                window.reloadCurrentChat();
            }
        });

        return true;
    }

    return false;
}
