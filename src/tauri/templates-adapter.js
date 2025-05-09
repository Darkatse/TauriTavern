/**
 * Templates Adapter for Tauri
 * 
 * This module adapts the SillyTavern templates system to use Tauri's file system API
 * for reading template files directly from the resources directory.
 */

// Check if we're running in a Tauri environment
const isTauri = window.__TAURI_INTERNALS__ !== undefined;

/**
 * Initialize the templates adapter
 * This function doesn't need to do anything special since the templates.js file
 * already checks for Tauri environment and uses the appropriate API.
 */
export function initializeTemplatesAdapter() {
    if (!isTauri) {
        return;
    }

    console.debug('Initializing Tauri templates adapter');
    
    // The actual adaptation is done in templates.js
    // This function is just a placeholder for future enhancements
}

export default {
    initializeTemplatesAdapter
};
