// Tauri Main Integration Module for SillyTavern
// This module initializes the Tauri integration when the application starts

import * as TauriBridge from './tauri-bridge.js';

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

// Initialize the Tauri integration
export async function initializeTauri() {
    if (!isTauri) {
        console.log('Not running in Tauri environment, skipping Tauri initialization');
        return false;
    }

    console.log('Initializing Tauri integration...');

    try {
        // Initialize the Tauri bridge
        const bridgeInitialized = await TauriBridge.initializeBridge();
        if (!bridgeInitialized) {
            console.error('Failed to initialize Tauri bridge');
            return false;
        }

        // Initialize the application
        const appInitialized = await TauriBridge.initializeApp();
        if (!appInitialized) {
            console.error('Failed to initialize Tauri application');
            return false;
        }

        console.log('Tauri integration initialized successfully');

        // Patch fetch and XHR for Tauri integration
        patchFetch();
        patchXHR();

        return true;
    } catch (error) {
        console.error('Error initializing Tauri integration:', error);
        return false;
    }
}

// Patch the fetch function to use Tauri API when appropriate
function patchFetch() {
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
                    ok: true,
                    json: async () => ({ token: 'tauri-dummy-token' })
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
function patchXHR() {
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

// Initialize Tauri when the module is loaded
initializeTauri().catch(error => {
    console.error('Failed to initialize Tauri:', error);
});
