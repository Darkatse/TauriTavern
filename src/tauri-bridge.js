// Tauri Bridge - Provides communication between SillyTavern frontend and Tauri backend

// Check if we're running in a Tauri environment
export const isTauriEnv = window.__TAURI_INTERNALS__ !== undefined;

// Export function to check if we're running in Tauri
export function isTauri() {
    return isTauriEnv;
}

// Import Tauri API if available
export let invoke = null;
export let listen = null;
export let convertFileSrc = null;

if (isTauriEnv) {
    // Import Tauri API
    const { invoke: tauriInvoke, convertFileSrc: tauriConvertFileSrc } = window.__TAURI__.core;
    const { listen: tauriListen } = window.__TAURI__.event;

    invoke = tauriInvoke;
    listen = tauriListen;
    convertFileSrc = tauriConvertFileSrc;

    console.log('Tauri environment detected. Using Tauri API.');
} else {
    console.log('Tauri environment not detected. Using fallback to server API.');
}

// Event listeners
const eventListeners = {};

// Register event listener
export function addEventListener(eventType, callback) {
    if (!eventListeners[eventType]) {
        eventListeners[eventType] = [];
    }

    eventListeners[eventType].push(callback);
    return () => removeEventListener(eventType, callback);
}

// Remove event listener
export function removeEventListener(eventType, callback) {
    if (!eventListeners[eventType]) {
        return;
    }

    const index = eventListeners[eventType].indexOf(callback);
    if (index !== -1) {
        eventListeners[eventType].splice(index, 1);
    }
}

// Initialize the bridge
export async function initializeBridge() {
    if (!isTauri) {
        return false;
    }

    try {
        // Check if backend is ready
        const isReady = await invoke('is_ready');

        if (isReady) {
            console.log('Tauri backend is ready.');

            // Listen for events from the backend
            listen('tauri-event', (event) => {
                const { event_type, data } = event.payload;

                // Dispatch event to listeners
                if (eventListeners[event_type]) {
                    eventListeners[event_type].forEach(callback => {
                        try {
                            callback(data);
                        } catch (error) {
                            console.error(`Error in event listener for ${event_type}:`, error);
                        }
                    });
                }
            });

            return true;
        } else {
            console.error('Tauri backend is not ready.');
            return false;
        }
    } catch (error) {
        console.error('Failed to initialize Tauri bridge:', error);
        return false;
    }
}

// Get CSRF token (dummy implementation for Tauri - not needed but kept for compatibility)
export async function getCsrfToken() {
    if (!isTauri) {
        // Use the original fetch for web version
        const tokenResponse = await fetch('/csrf-token');
        const tokenData = await tokenResponse.json();
        return tokenData.token;
    }

    // In Tauri, we don't need CSRF protection as we're not using cookies
    // But we return a dummy token for compatibility
    return 'tauri-dummy-token';
}

// Initialize the application
export async function initializeApp() {
    if (!isTauri) {
        return false;
    }

    try {
        // Initialize data directories
        console.log('Initializing application data...');

        // 不再直接调用 getSettings，而是通过 invoke 获取版本信息来检查后端是否准备好
        const version = await invoke('get_version');
        console.log('Application initialized, version:', version);

        return true;
    } catch (error) {
        console.error('Failed to initialize application:', error);
        return false;
    }
}

// Note: All API functions have been moved to their respective modules
// in the src/scripts/tauri/ directory. Please use those modules instead.
// For example:
// - Characters API: src/scripts/tauri/characters-api.js
// - Chats API: src/scripts/tauri/chats-api.js
// - Settings API: src/scripts/tauri/settings-api.js
// - User API: src/scripts/tauri/user-api.js (to be implemented)

// This file now only provides core Tauri functionality:
// - invoke: for calling Tauri commands
// - listen: for listening to Tauri events
// - convertFileSrc: for converting file paths to URLs
// - isTauri: for checking if we're running in Tauri
// - initializeBridge: for initializing the bridge
// - getCsrfToken: for getting a CSRF token (dummy in Tauri)
// - initializeApp: for initializing the application
// - getAssetUrl: for getting a URL for an asset

// Utility functions
export async function getVersion() {
    if (!isTauri) {
        // Fallback to server API
        const response = await fetch('/version');
        return response.json();
    }

    return invoke('get_version');
}

/**
 * Get detailed client version information
 * @returns {Promise<Object>} Version information object
 */
export async function getClientVersion() {
    if (!isTauri) {
        // Fallback to server API
        const response = await fetch('/version');
        return response.json();
    }

    try {
        console.log('Getting client version from Tauri backend');
        const versionInfo = await invoke('get_client_version');
        console.log('Client version from Tauri backend:', versionInfo);
        return versionInfo;
    } catch (error) {
        console.error('Error getting client version from Tauri backend:', error);
        // Fallback to basic version if detailed version fails
        const version = await invoke('get_version');
        return {
            agent: `TauriTavern/${version}`,
            pkgVersion: version,
            gitRevision: 'tauri',
            gitBranch: 'main'
        };
    }
}

// Convert a file path to a URL that can be used in the frontend
/**
 * @param {string} path - The file path to convert
 * @returns {string} - The converted URL
 */
export function getAssetUrl(path) {
    if (!isTauri || !convertFileSrc) {
        // Fallback to server path
        return path;
    }

    return convertFileSrc(path);
}

// Import and initialize adapters
import { initializeExtensionsAdapter } from './tauri/extensions-adapter.js';
import { initializeAvatarsAdapter } from './tauri/avatars-adapter.js';
import { initializeTemplatesAdapter } from './tauri/templates-adapter.js';
import { initializeCharactersAdapter } from './tauri/characters-adapter.js';
import { initializeChatsAdapter } from './tauri/chats-adapter.js';
import { initializeGroupsAdapter } from './tauri/groups-adapter.js';
import { initializeBackgroundsAdapter } from './tauri/backgrounds-adapter.js';
import { initializeThemesAdapter } from './tauri/themes-adapter.js';

// Initialize the bridge when the script is loaded
initializeBridge().then(() => {
    if (isTauriEnv) {
        // Initialize Tauri adapters
        initializeExtensionsAdapter();
        initializeAvatarsAdapter();
        initializeTemplatesAdapter();
        initializeCharactersAdapter();
        initializeChatsAdapter();
        initializeGroupsAdapter();
        initializeBackgroundsAdapter();
        initializeThemesAdapter();
    }
}).catch(error => {
    console.error('Failed to initialize Tauri bridge:', error);
});
